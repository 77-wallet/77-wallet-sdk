# Legacy Transaction Worker Retirement Assertion Matrix

| Concern | Expected result | Evidence |
| --- | --- | --- |
| Historical recovery | The deleted workers remain readable at the archive branch | Archive ref resolves to the recorded commit |
| Shadow startup | Collect, fee, and withdraw handles still initialize their Shadow actor systems | `wallet-api` library compilation |
| Fast-path access | Existing handle getters return the same Shadow actor-system types | `wallet-api` library compilation |
| Same-account serialization | A second permit waits until the first permit is released | Address-lock unit test |
| Different-account concurrency | Different account permits can coexist | Address-lock unit test |
| Runtime behavior | No backend contract, persisted fact, state transition, or public SDK type changes | Final diff review |
| Scanner scope | No transaction `legacy` module remains under `api_trans` | Source-tree reference scan |
