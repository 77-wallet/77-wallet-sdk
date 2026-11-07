mod support;

use support::CreationScaleProbe;

// Standalone synthetic offline experiment, not a real device/recovery benchmark.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "standalone 500k-address DB experiment; run exact with isolated TMPDIR"]
async fn wallet_creation_database_scale_probe() {
    // Given an offline Manager/fake-backend fixture in an isolated temp root.
    let mut probe = CreationScaleProbe::given().await;
    probe.given_address_count(100).await;
    probe.when_create_pairs("warmup", false, 1).await;
    // When creating both wallet types with small, then 500k idle/contended data.
    probe.when_create_pairs("small", false, 3).await;
    probe.given_address_count(500_000).await;
    probe.when_create_pairs("large_idle", false, 3).await;
    probe.when_create_pairs("large_contended", true, 3).await;
    // Then failure is logged without changing existing partial-persistence semantics,
    // and all background address identities survive.
    probe.when_bind_info_fails().await;
    probe.then_background_unchanged().await;
}
