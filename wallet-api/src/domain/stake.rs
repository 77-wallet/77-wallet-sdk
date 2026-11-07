use wallet_chain_interact::{
    tron::{
        TronChain,
        operations::{
            TronSimulateOperation, TronTxOperation,
            stake::{self, DelegatedResource},
        },
        params::ResourceConsumer,
    },
    types::MultisigTxResp,
};

pub(crate) mod batch;

use crate::response_vo::standard_wallet::stake::VoteListResp;

pub enum StakeArgs {
    // 质押
    Freeze(stake::FreezeBalanceArgs),
    // 解冻
    UnFreeze(stake::UnFreezeBalanceArgs),
    // 取消解锁
    CancelAllUnFreeze(stake::CancelAllFreezeBalanceArgs),
    // 提取可以
    Withdraw(stake::WithdrawUnfreezeArgs),
    // 代理
    Delegate(stake::DelegateArgs),
    // 批量代理
    BatchDelegate(Vec<stake::DelegateArgs>),
    // 取消代理
    UnDelegate(stake::UnDelegateArgs),
    // 批量取消代理
    BatchUnDelegate(Vec<stake::UnDelegateArgs>),
    // 投票
    Votes(stake::VoteWitnessArgs),
    // 提币
    WithdrawReward(stake::WithdrawBalanceArgs),
}

impl StakeArgs {
    pub async fn exec(
        self,
        account: &str,
        chain: &TronChain,
    ) -> Result<ResourceConsumer, crate::error::service::ServiceError> {
        let signature_num = 1;
        let res = match self {
            Self::Freeze(params) => chain.simple_fee(account, signature_num, params).await?,
            Self::UnFreeze(params) => chain.simple_fee(account, signature_num, params).await?,
            Self::CancelAllUnFreeze(params) => {
                chain.simple_fee(account, signature_num, params).await?
            }
            Self::Withdraw(params) => chain.simple_fee(account, signature_num, params).await?,
            Self::Delegate(params) => {
                chain.simulate_simple_fee(account, "", signature_num, params).await?
            }
            Self::UnDelegate(params) => {
                chain.simulate_simple_fee(account, "", signature_num, params).await?
            }
            Self::BatchDelegate(mut params) => {
                let item = params.remove(0);

                let mut consumer =
                    chain.simulate_simple_fee(account, "", signature_num, item).await?;

                for item in params {
                    let raw_data_hex = item.simulate_raw_transaction()?;

                    let size = chain.provider.calc_bandwidth(&raw_data_hex, signature_num);
                    consumer.bandwidth.consumer += size;
                }
                consumer
            }
            Self::BatchUnDelegate(mut params) => {
                let item = params.remove(0);

                let mut consumer =
                    chain.simulate_simple_fee(account, "", signature_num, item).await?;

                for item in params {
                    let raw_data_hex = item.simulate_raw_transaction()?;

                    let size = chain.provider.calc_bandwidth(&raw_data_hex, signature_num);
                    consumer.bandwidth.consumer += size;
                }
                consumer
            }

            Self::Votes(params) => chain.simple_fee(account, signature_num, params).await?,
            Self::WithdrawReward(params) => {
                chain.simple_fee(account, signature_num, params).await?
            }
        };
        Ok(res)
    }

    pub fn get_to(&self) -> String {
        match self {
            Self::Freeze(params) => params.get_to(),
            Self::UnFreeze(params) => params.get_to(),
            Self::CancelAllUnFreeze(params) => params.get_to(),
            Self::Withdraw(params) => params.get_to(),
            Self::Delegate(params) => params.get_to(),
            Self::UnDelegate(params) => params.get_to(),
            Self::BatchDelegate(_params) => String::new(),
            Self::BatchUnDelegate(_params) => String::new(),
            Self::Votes(params) => params.get_to(),
            Self::WithdrawReward(params) => params.get_to(),
        }
    }

    // 构建多签交易
    pub async fn build_multisig_tx(
        self,
        chain: &TronChain,
        expiration: u64,
    ) -> Result<MultisigTxResp, crate::error::service::ServiceError> {
        let res = match self {
            Self::Freeze(params) => chain.build_multisig_transaction(params, expiration).await?,
            Self::UnFreeze(params) => chain.build_multisig_transaction(params, expiration).await?,
            Self::CancelAllUnFreeze(params) => {
                chain.build_multisig_transaction(params, expiration).await?
            }
            Self::Withdraw(params) => chain.build_multisig_transaction(params, expiration).await?,
            Self::Delegate(params) => chain.build_multisig_transaction(params, expiration).await?,
            Self::UnDelegate(params) => {
                chain.build_multisig_transaction(params, expiration).await?
            }
            Self::Votes(params) => chain.build_multisig_transaction(params, expiration).await?,
            Self::WithdrawReward(params) => {
                chain.build_multisig_transaction(params, expiration).await?
            }
            _ => {
                return Err(crate::error::business::BusinessError::Stake(
                    crate::error::business::stake::StakeError::MultisigUnSupportBillKind,
                ))?;
            }
        };
        Ok(res)
    }
}

pub(crate) struct StakeDomain;

#[derive(Debug)]
pub(crate) struct Representative {
    votes: f64, // 投票数量
    apr: f64,   // 年化收益率（APR）
}

impl Representative {
    pub(crate) fn new(votes: f64, apr: f64) -> Self {
        Self { votes, apr }
    }
}

const SR_ONE_DAY_TOTAL_REWARD: f64 = 460_800.0;
const VOTE_ONE_DAY_TOTAL_REWARD: f64 = 4_608_000.0;
const VOTER_VOTES: f64 = 10_000_000.0;

impl StakeDomain {
    pub async fn get_delegate_info(
        ctx: &'static crate::context::Context,
        from: &str,
        to: &str,
        chain: &TronChain,
    ) -> Result<DelegatedResource, crate::error::service::ServiceError> {
        let key = format!("{}_{}_delegate", from, to);

        // 现从缓存中获取数据、没有在从链上获取
        let cache = ctx.get_global_cache();
        let res: Option<DelegatedResource> = cache.get(&key).await;
        match res {
            Some(res) => {
                // let res = serde_from_value::<DelegatedResource>(res.data)?;
                Ok(res)
            }
            None => {
                let res = chain.provider.delegated_resource(from, to).await?;
                cache.set_with_expiration(&key, &res, 30).await?;
                Ok(res)
            }
        }
    }

    // Function to calculate the voter reward
    pub(crate) fn calculate_vote_reward(sr_votes: f64, total_sr_votes: f64, brokerage: f64) -> f64 {
        VOTE_ONE_DAY_TOTAL_REWARD
            * (sr_votes / total_sr_votes)
            * brokerage
            * (VOTER_VOTES / sr_votes)
    }

    pub(crate) fn calculate_block_reward(brokerage: f64, sr_votes: f64) -> f64 {
        SR_ONE_DAY_TOTAL_REWARD / 27.0 * brokerage * VOTER_VOTES / sr_votes
    }

    // Function to calculate APR
    pub(crate) fn calculate_apr(voter_reward: f64, block_reward: f64) -> f64 {
        // if voter_votes == 0.0 {
        //     return 0.0;
        // }
        ((voter_reward + block_reward) / VOTER_VOTES) * 100.0 * 365.0
    }

    pub(crate) fn calculate_comprehensive_apr(representatives: Vec<Representative>) -> f64 {
        // 如果没有代表，返回 0
        if representatives.is_empty() {
            return 0.0;
        }

        // 使用迭代器计算加权总和和总投票数
        let (weighted_sum, total_votes): (f64, f64) = representatives
            .iter()
            .fold((0.0, 0.0), |acc, rep| (acc.0 + rep.votes * rep.apr, acc.1 + rep.votes));

        // 防止除以 0 的情况
        if total_votes == 0.0 { 0.0 } else { weighted_sum / total_votes }
    }

    // 从后端获取代表列表
    pub(crate) async fn vote_list_from_backend(
        ctx: &'static crate::context::Context,
    ) -> Result<VoteListResp, crate::error::service::ServiceError> {
        let backend = ctx.get_global_backend_api();
        let mut list = backend.vote_list().await?;
        // let witness_list = list.node_resp_list;
        list.node_resp_list.iter_mut().for_each(|item| {
            item.brokerage = (100.0 - item.brokerage) / 100.0;
            item.apr *= 100.0;
        });
        // list.node_resp_list.sort_by(|a, b| {
        //     let a_vote_count = a.vote_count;
        //     let b_vote_count = b.vote_count;
        //     b_vote_count.cmp(&a_vote_count)
        // });

        let res = VoteListResp {
            total: list.total_node,
            total_votes: list.total_vote_count,
            data: list.node_resp_list.into_iter().map(|node| node.into()).collect(),
        };

        Ok(res)
    }

    // sdk主动查询代表列表
    #[allow(unused)]
    pub(crate) async fn vote_list(
        chain: &wallet_chain_interact::tron::Provider,
    ) -> Result<VoteListResp, crate::error::service::ServiceError> {
        let mut witness_list = chain.list_witnesses().await?.witnesses;
        witness_list.sort_by(|a, b| {
            let a_vote_count = a.vote_count;
            let b_vote_count = b.vote_count;
            b_vote_count.cmp(&a_vote_count)
        });
        let total = witness_list.len() as u16;
        let total_sr_votes = witness_list.iter().map(|w| w.vote_count).sum::<i64>();

        let mut data = Vec::new();
        for (i, witness) in witness_list.into_iter().enumerate() {
            if i == 26 {
                break;
            }
            let wallet_chain_interact::tron::operations::stake::Witness {
                address,
                vote_count,
                url,
                ..
            } = witness;
            let brokerage = (100.0 - chain.get_brokerage(&address).await?.brokerage as f64) / 100.0;

            let block_reward = StakeDomain::calculate_block_reward(brokerage, vote_count as f64);

            let vote_reward = StakeDomain::calculate_vote_reward(
                vote_count as f64,
                total_sr_votes as f64,
                brokerage,
            );
            let apr = StakeDomain::calculate_apr(vote_reward, block_reward);

            data.push(crate::response_vo::standard_wallet::stake::Witness::new(
                None,
                &wallet_utils::address::hex_to_bs58_addr(&address)?,
                vote_count,
                &url,
                brokerage,
                apr,
            ));
        }
        let res = VoteListResp { total, total_votes: total_sr_votes, data };
        Ok(res)
    }
}

#[cfg(test)]
mod cal_tests {
    use crate::domain::stake::StakeDomain;

    // Function to calculate the voter reward
    // fn calculate_voter_reward(
    //     total_reward: f64,
    //     voter_votes: f64,
    //     sr_votes: f64,
    //     total_sr_votes: f64,
    //     voter_share: f64,
    // ) -> f64 {
    //     total_reward * (sr_votes / total_sr_votes) * voter_share * (voter_votes / sr_votes)
    // }

    // fn calculate_block_reward(voter_share: f64, voter_votes: f64, sr_votes: f64) -> f64 {
    //     460800.0 / 27.0 * voter_share * voter_votes / sr_votes
    // }

    // Function to calculate APR
    // fn calculate_apr(voter_reward: f64, block_reward: f64, voter_votes: f64) -> f64 {
    //     if voter_votes == 0.0 {
    //         return 0.0;
    //     }
    //     ((voter_reward + block_reward) / voter_votes) * 100.0 * 365.0
    // }

    #[test]
    fn test_calculate_apr() {
        // Parameters for the test case
        let _total_reward = 4_608_000.0; // Total reward pool

        let total_sr_votes = 39806518656.0; // Total votes of all SR and SRP

        // let sr_votes = 3069539068.0; // Votes obtained by the SR
        let _sr_votes = 1248080337.0; // Votes obtained by the SR
        let sr_votes = 3071535822.0; // Votes obtained by the SR

        let _voter_votes = 10_000_000.0; // Voter's votes
        let voter_share = 1.0; // Voter share (80%)
        // let voter_share = 0.90; // Voter share (80%)

        // Calculate voter reward
        let voter_reward =
            StakeDomain::calculate_vote_reward(sr_votes, total_sr_votes, voter_share);

        let block_reward = StakeDomain::calculate_block_reward(voter_share, sr_votes);
        println!("block reward: {}", block_reward);
        // Calculate APR
        let apr = StakeDomain::calculate_apr(voter_reward, block_reward);

        // Debug output
        println!("Voter Reward: {:.2}", voter_reward);
        println!("Voter APR: {:.2}", apr);

        // Assert results (expected values based on current formula/inputs)
        assert!((voter_reward - 1157.60).abs() < 1e-2); // Reward should be close to 1157.60 TRX
        assert!((apr - 4.43).abs() < 1e-2); // APR should be close to 4.43%
    }
}

pub struct EstimateTxConsumer {
    pub bandwidth: i64,
    pub energy: i64,
}
impl EstimateTxConsumer {
    // 获取交易需要消耗的资源，TODO: 根据不同的网络来获取对应的代币地址
    pub async fn new(_chain: &TronChain) -> Result<Self, crate::error::service::ServiceError> {
        // let contract = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t";
        // let from = "TFrszR3Bz1HUaEjL9ES1m424e5NJDsFsfT";
        // let to = "TTofbJMU2iMRhA39AJh51sYvhguWUnzeB1";
        // let value = unit::convert_to_u256("1", 6)?;

        Ok(Self { bandwidth: 268, energy: 64285 })

        // let params =
        //     operations::transfer::ContractTransferOpt::new(&contract, from, to, value, None)?;

        // let res = params.constant_contract(chain.get_provider()).await?;

        // Ok(Self {
        //     bandwidth: 268.0,
        //     energy: res.energy_used as f64,
        // })
    }
}
