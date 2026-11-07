use super::*;
use std::{
    sync::Mutex,
    task::{Context, Poll, Wake, Waker},
};

struct NoopWake;
impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}
fn run<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(NoopWake));
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("test adapter must complete immediately"),
    }
}

struct RecordingProvider {
    calls: Mutex<Vec<(bool, Vec<u8>, Vec<u8>)>>,
    failure: Option<ProtectionError>,
}
impl DeviceProtectionProvider for RecordingProvider {
    fn protect<'a>(
        &'a self,
        context: &'a ProtectionContext,
        data: &'a [u8],
    ) -> ProtectionFuture<'a> {
        self.calls.lock().unwrap().push((true, context.authenticated_data(), data.to_vec()));
        Box::pin(async move {
            if let Some(error) = self.failure {
                return Err(error);
            }
            // Contract fixture, NOT cryptography.
            Ok(b"opaque-adapter-ciphertext".to_vec())
        })
    }
    fn unprotect<'a>(
        &'a self,
        context: &'a ProtectionContext,
        data: &'a [u8],
    ) -> ProtectionFuture<'a> {
        self.calls.lock().unwrap().push((false, context.authenticated_data(), data.to_vec()));
        Box::pin(async move {
            if let Some(error) = self.failure {
                return Err(error);
            }
            Ok(b"inner-password-ciphertext".to_vec())
        })
    }
}
fn context() -> ProtectionContext {
    ProtectionContext::new("wallet-a", "seed").unwrap()
}

#[test]
fn missing_provider_fails_both_operations() {
    let service = DeviceProtection::default();
    assert_eq!(run(service.protect(&context(), b"inner")), Err(ProtectionError::NotConfigured));
    assert_eq!(run(service.unprotect(&context(), b"outer")), Err(ProtectionError::NotConfigured));
}
#[test]
fn forwards_bound_context_and_returns_only_provider_output() {
    let provider = Arc::new(RecordingProvider { calls: Mutex::new(Vec::new()), failure: None });
    let service = DeviceProtection::new(provider.clone());
    assert_eq!(
        run(service.protect(&context(), b"inner")),
        Ok(b"opaque-adapter-ciphertext".to_vec())
    );
    assert_eq!(
        run(service.unprotect(&context(), b"outer")),
        Ok(b"inner-password-ciphertext".to_vec())
    );
    let calls = provider.calls.lock().unwrap();
    assert_eq!(
        *calls,
        vec![
            (true, context().authenticated_data(), b"inner".to_vec()),
            (false, context().authenticated_data(), b"outer".to_vec())
        ]
    );
}
#[test]
fn errors_propagate_without_returning_input() {
    for error in [
        ProtectionError::Unavailable,
        ProtectionError::Locked,
        ProtectionError::Cancelled,
        ProtectionError::KeyMissing,
        ProtectionError::KeyInvalidated,
        ProtectionError::AuthenticationFailed,
        ProtectionError::MalformedCiphertext,
        ProtectionError::Timeout,
        ProtectionError::PlatformFailure,
    ] {
        let service = DeviceProtection::new(Arc::new(RecordingProvider {
            calls: Mutex::new(Vec::new()),
            failure: Some(error),
        }));
        assert_eq!(run(service.protect(&context(), b"inner")), Err(error));
        assert_eq!(run(service.unprotect(&context(), b"outer")), Err(error));
    }
}
#[test]
fn context_encoding_is_unambiguous_and_rejects_empty_fields() {
    assert!(ProtectionContext::new("", "seed").is_err());
    assert!(ProtectionContext::new("wallet", "").is_err());
    let a = ProtectionContext::new("a/b", "c").unwrap();
    let b = ProtectionContext::new("a", "b/c").unwrap();
    assert_ne!(a.authenticated_data(), b.authenticated_data());
    assert_ne!(
        context().authenticated_data(),
        ProtectionContext::new("wallet-b", "seed").unwrap().authenticated_data()
    );
    assert_ne!(
        context().authenticated_data(),
        ProtectionContext::new("wallet-a", "phrase").unwrap().authenticated_data()
    );
}

struct DeferredProvider;
impl DeviceProtectionProvider for DeferredProvider {
    fn protect<'a>(&'a self, _: &'a ProtectionContext, _: &'a [u8]) -> ProtectionFuture<'a> {
        let mut ready = false;
        Box::pin(std::future::poll_fn(move |cx| {
            if ready {
                Poll::Ready(Ok(b"native-result".to_vec()))
            } else {
                ready = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }))
    }
    fn unprotect<'a>(
        &'a self,
        context: &'a ProtectionContext,
        data: &'a [u8],
    ) -> ProtectionFuture<'a> {
        self.protect(context, data)
    }
}
#[test]
fn pending_native_callback_yields_before_returning_result() {
    let service = DeviceProtection::new(Arc::new(DeferredProvider));
    let context = context();
    let mut future = Box::pin(service.protect(&context, b"inner"));
    let waker = Waker::from(Arc::new(NoopWake));
    let mut cx = Context::from_waker(&waker);
    assert!(future.as_mut().poll(&mut cx).is_pending());
    assert_eq!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(b"native-result".to_vec())));
}

#[test]
fn provider_identity_distinguishes_clone_replacement_and_disabled_mode() {
    struct Never;
    impl DeviceProtectionProvider for Never {
        fn protect<'a>(&'a self, _: &'a ProtectionContext, _: &'a [u8]) -> ProtectionFuture<'a> {
            Box::pin(async { Err(ProtectionError::Unavailable) })
        }
        fn unprotect<'a>(&'a self, _: &'a ProtectionContext, _: &'a [u8]) -> ProtectionFuture<'a> {
            Box::pin(async { Err(ProtectionError::Unavailable) })
        }
    }
    let configured = DeviceProtection::new(std::sync::Arc::new(Never));
    assert!(configured.same_provider(&configured.clone()));
    assert!(!configured.same_provider(&DeviceProtection::new(std::sync::Arc::new(Never))));
    assert!(!configured.same_provider(&DeviceProtection::default()));
    assert!(!DeviceProtection::default().same_provider(&configured));
    assert!(DeviceProtection::default().same_provider(&DeviceProtection::default()));
}

#[test]
fn provisioning_is_explicit_and_unsupported_by_default() {
    assert_eq!(
        run(DeviceProtection::default().provision_for_new_storage()),
        Err(ProtectionError::NotConfigured)
    );
    let provider = Arc::new(RecordingProvider { calls: Mutex::new(Vec::new()), failure: None });
    let protection = DeviceProtection::new(provider.clone());
    assert_eq!(run(protection.provision_for_new_storage()), Err(ProtectionError::Unavailable));
    assert!(provider.calls.lock().unwrap().is_empty());
}
