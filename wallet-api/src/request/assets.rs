type ChainId = String;
type Currency = String;

#[derive(serde::Deserialize, serde::Serialize, PartialEq, Debug)]
pub enum GetChain {
    All,
    One(ChainId, Currency),
}
