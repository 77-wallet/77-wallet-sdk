use crate::{
    context::Context,
    domain::{
        bill::BillDomain,
        chain::{adapter::ChainAdapterFactory, transaction::ChainTransDomain},
    },
    error::{
        business::{BusinessError, chain::ChainError, stake::StakeError},
        service::ServiceError,
    },
    messaging::notify::{
        FrontendNotifyEvent,
        event::NotifyEvent,
        other::{Process, TransactionProcessFrontend},
    },
    request::{
        stake::{BatchDelegate, BatchUnDelegate},
        transaction::Signer,
    },
    response_vo::standard_wallet::stake::{BatchDelegateResp, BatchRes, ResourceResp},
};
use wallet_chain_interact::{
    BillResourceConsume,
    tron::{
        TronChain,
        operations::{
            RawTransactionParams, TronTxOperation,
            stake::{DelegateArgs, ResourceType, UnDelegateArgs},
        },
        params::{Resource as TronResource, ResourceConsumer},
        protocol::{ChainParameter, account::AccountResourceDetail},
    },
    types::ChainPrivateKey,
};
use wallet_database::{
    entities::bill::{BillExtraResourceValue, BillKind},
    repositories::bill::BillRepo,
};
use wallet_types::constant::chain_code;

struct TempBuildTransaction {
    to: String,
    value: f64,
    resource_value: i64,
    consumer: ResourceConsumer,
    raw_data: RawTransactionParams,
}

pub(crate) struct StakeBatchDomain {
    ctx: &'static Context,
    chain: TronChain,
}

impl StakeBatchDomain {
    pub(crate) async fn new(ctx: &'static Context) -> Result<Self, ServiceError> {
        Ok(Self { ctx, chain: ChainAdapterFactory::get_tron_adapter_with_ctx(ctx).await? })
    }

    async fn get_key(
        &self,
        from: &str,
        signer: &Option<Signer>,
        password: &str,
    ) -> Result<ChainPrivateKey, ServiceError> {
        ChainTransDomain::get_key(self.ctx, from, chain_code::TRON, password, signer).await
    }

    async fn batch_build_transaction<T>(
        &self,
        args: Vec<impl TronTxOperation<T>>,
        bill_kind: BillKind,
        chain_param: &ChainParameter,
        resource: &AccountResourceDetail,
        resource_type: ResourceType,
    ) -> Result<Vec<TempBuildTransaction>, ServiceError> {
        let mut result = vec![];
        for (i, item) in args.into_iter().enumerate() {
            let data = NotifyEvent::TransactionProcess(TransactionProcessFrontend::new_with_num(
                bill_kind,
                (i + 1) as i64,
                Process::Building,
            ));
            FrontendNotifyEvent::new(data).send_with_ctx(self.ctx).await?;
            let res = item.build_raw_transaction(&self.chain.provider).await?;
            let bandwidth = self.chain.provider.calc_bandwidth(&res.raw_data_hex, 1);
            let bandwidth = TronResource::new(
                resource.available_bandwidth(),
                bandwidth,
                chain_param.get_transaction_fee(),
                "bandwidth",
            );
            let resource_value =
                resource.resource_value(resource_type, item.get_value() as i64)? as i64;
            result.push(TempBuildTransaction {
                raw_data: res,
                to: item.get_to(),
                value: item.get_value(),
                resource_value,
                consumer: ResourceConsumer::new(bandwidth, None),
            });
        }
        Ok(result)
    }

    async fn batch_exec(
        &self,
        from: &str,
        key: ChainPrivateKey,
        bill_kind: BillKind,
        txs: Vec<TempBuildTransaction>,
        resource_type: ResourceType,
    ) -> Result<(Vec<BatchRes>, Vec<String>), ServiceError> {
        let mut exec_res = vec![];
        let mut tx_hash = vec![];
        for (i, item) in txs.into_iter().enumerate() {
            let data = NotifyEvent::TransactionProcess(TransactionProcessFrontend::new_with_num(
                bill_kind,
                (i + 1) as i64,
                Process::Broadcast,
            ));
            FrontendNotifyEvent::new(data).send_with_ctx(self.ctx).await?;
            match self.chain.exec_transaction_v1(item.raw_data, key.clone()).await {
                Ok(hash) => {
                    let bill_consumer =
                        BillResourceConsume::new_tron(item.consumer.act_bandwidth() as u64, 0)
                            .to_json_str()?;
                    let extra = BillExtraResourceValue {
                        value: item.resource_value,
                        resource_type: resource_type.to_string(),
                    };
                    let entity = BillRepo::build_stake_bill(
                        hash.clone(),
                        from.to_string(),
                        item.to.clone(),
                        item.value,
                        bill_kind,
                        bill_consumer,
                        item.consumer.transaction_fee(),
                        Some(extra),
                    );
                    BillDomain::create_bill(&self.ctx.core_pool()?, entity).await?;
                    exec_res.push(BatchRes { address: item.to, status: true });
                    tx_hash.push(hash);
                }
                Err(_) => exec_res.push(BatchRes { address: item.to, status: false }),
            }
        }
        Ok((exec_res, tx_hash))
    }

    async fn process_batch<T>(
        &self,
        resource_type: ResourceType,
        owner_address: &str,
        bill_kind: BillKind,
        password: &str,
        args: Vec<impl TronTxOperation<T>>,
        amount: i64,
        signer: &Option<Signer>,
    ) -> Result<BatchDelegateResp, ServiceError> {
        let resource = ChainTransDomain::account_resource(&self.chain, owner_address).await?;
        let chain_param = self.chain.get_provider().chain_params().await?;
        let txs = self
            .batch_build_transaction(args, bill_kind, &chain_param, &resource, resource_type)
            .await?;
        let balance = self.chain.balance(owner_address, None).await?;
        let fee = txs.iter().map(|item| item.consumer.transaction_fee_i64()).sum::<i64>();
        if balance.to::<i64>() < fee {
            return Err(ServiceError::Business(BusinessError::Chain(
                ChainError::InsufficientFeeBalance,
            )));
        }
        let key = self.get_key(owner_address, signer, password).await?;
        let res = self.batch_exec(owner_address, key, bill_kind, txs, resource_type).await?;
        let resource_value = resource.resource_value(resource_type, amount)?;
        Ok(BatchDelegateResp::new(
            owner_address.to_string(),
            res,
            ResourceResp::new(amount, resource_type, resource_value),
            bill_kind,
        ))
    }

    pub(crate) async fn batch_delegate(
        &self,
        req: BatchDelegate,
        password: String,
    ) -> Result<BatchDelegateResp, ServiceError> {
        let resource_type = ResourceType::try_from(req.resource_type.as_str())?;

        for list in req.list.iter() {
            if list.value <= 0 {
                return Err(ServiceError::Business(BusinessError::Stake(
                    StakeError::DelegateLessThanMin,
                )));
            }
        }

        let bill_kind = match resource_type {
            ResourceType::BANDWIDTH => BillKind::BatchDelegateBandwidth,
            ResourceType::ENERGY => BillKind::BatchDelegateEnergy,
        };

        let args: Vec<DelegateArgs> = (&req).try_into()?;
        let amount = req.total();

        self.process_batch(
            resource_type,
            &req.owner_address,
            bill_kind,
            &password,
            args,
            amount,
            &req.signer,
        )
        .await
    }

    pub(crate) async fn batch_un_delegate(
        &self,
        req: BatchUnDelegate,
        password: String,
    ) -> Result<BatchDelegateResp, ServiceError> {
        let resource_type = ResourceType::try_from(req.resource_type.as_str())?;

        for list in req.list.iter() {
            if list.value <= 0 {
                return Err(ServiceError::Business(BusinessError::Stake(
                    StakeError::UnDelegateLessThanMin,
                )));
            }
        }

        let bill_kind = match resource_type {
            ResourceType::BANDWIDTH => BillKind::BatchUnDelegateBandwidth,
            ResourceType::ENERGY => BillKind::BatchUnDelegateEnergy,
        };

        let args: Vec<UnDelegateArgs> = (&req).try_into()?;
        let amount = req.total();

        self.process_batch(
            resource_type,
            &req.owner_address,
            bill_kind,
            &password,
            args,
            amount,
            &req.signer,
        )
        .await
    }
}
