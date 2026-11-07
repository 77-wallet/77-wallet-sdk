use crate::get_manager;
use anyhow::Result;
use wallet_api::request::stake::{VoteWitnessReq, VotesReq, WithdrawBalanceReq};
use wallet_database::entities::bill::BillKind;

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_vote() -> Result<()> {
    let manager = get_manager().await;
    let owner_address = "TMrVocuPpNqf3fpPSSWy7V8kyAers3p1Jc";
    let vote_witness_req = VoteWitnessReq::new(
        owner_address,
        vec![
            VotesReq::new("TEp1ru7opCexkbFM9ChK6DFfL2XFSfUo2N", 20, "name"),
            VotesReq::new("TA4pHhHgobzSGH3CWPsZ5URNk3QkzUEggX", 20, "name"),
        ],
        None,
    );
    let password = "123456"; // Replace with the actual password

    let res = manager.votes(vote_witness_req, password).await?;

    tracing::info!("votes {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_votes_fee() -> Result<()> {
    let manager = get_manager().await;
    let vote_address = "TA4pHhHgobzSGH3CWPsZ5URNk3QkzUEggX";
    let vote_count = 1;
    let votes = VotesReq::new(vote_address, vote_count, "name");
    let owner_address = "TFdDqaoMkPbWWv9EUTbmfGP142f9ysiJq2";
    let req = VoteWitnessReq::new(owner_address, vec![votes], None);

    let bill_kind = BillKind::Vote.to_i8() as i64;

    let content = serde_json::to_string(&req).unwrap();
    let res = manager.estimate_stake_fee(bill_kind, content).await?;

    tracing::info!("fee {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_withdraw_fee() -> Result<()> {
    let manager = get_manager().await;

    let owner_address = "TGtSVaqXzzGM2XgbUvgZzZeNqFwp1VvyXS";
    let req = WithdrawBalanceReq::new(owner_address, None);

    let bill_kind = BillKind::WithdrawReward.to_i8() as i64;

    let content = serde_json::to_string(&req).unwrap();
    let res = manager.estimate_stake_fee(bill_kind, content).await?;

    tracing::info!("fee {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded stake wallet"]
async fn test_claim_votes() -> Result<()> {
    let manager = get_manager().await;

    let owner_address = "TFzMRRzQFhY9XFS37veoswLRuWLNtbyhiB";
    let req = WithdrawBalanceReq::new(owner_address, None);

    let res = manager.claim_votes_rewards(req, "123456").await?;

    tracing::info!("fee {}", serde_json::to_string(&res).unwrap());
    Ok(())
}
