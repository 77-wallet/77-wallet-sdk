use strum_macros::Display;

#[derive(
    sqlx::Type,
    Debug,
    Clone,
    Copy,
    serde_repr::Deserialize_repr,
    serde_repr::Serialize_repr,
    PartialEq,
    Eq,
    PartialOrd,
    Display,
)]
#[repr(u8)]
pub enum ApiTradeType {
    Withdraw = 1,     // 提币
    Collect = 2,      // 归集
    TransferFee = 3,  // 归集手续费
    SelfWithdraw = 4, // 手动提币
    SelfRecharge = 5, // 充值
}
