# 设备保护接口：第一轮可行性验证

日期：2026-09-12。状态：仅验证接口形态，未接入生产 SDK 或 SEP/TEE。

## 已验证

临时程序位于 `target/device-protection-spike/`，不会作为生产代码提交。采用 `Arc<dyn DeviceProtection>` 注入异步 `protect(context, bytes)` / `unprotect(context, blob)`；示例存储层负责文件读写。输入仅为合成的 keystore 字节，不含真实钱包数据。

软件 AES-GCM 适配器仅用于验证契约，不具备硬件不可导出性，不应作为生产降级方案。

- 实际文件写入外层密文；重新构造存储对象并使用同一适配器可读回原字节。此项不是进程重启后的硬件密钥持久化测试。
- 同一输入重复加密使用新 nonce，密文不同。
- 不同适配器密钥、不同用途标识、密文篡改均被认证拒绝。
- 适配器失败或超时不覆盖已有文件；读取失败不会退回原始字节。
- 旧格式被拒绝，正式接入需要显式升级流程。

## 验证命令与结果

```sh
CARGO_TARGET_DIR=target/device-protection-spike/build cargo test --offline --manifest-path target/device-protection-spike/Cargo.toml
cargo fmt --manifest-path target/device-protection-spike/Cargo.toml -- --check
```

4 passed, 0 failed。无网络、无真实钱包访问。临时程序在 target 清理后消失。

## 正式接入的位置

- SDK：Context 中注入保护接口；上层存储流程等待返回后再写入文件或 SQLite BLOB。
- 插件：平台适配器实现设备密钥操作，初始化时提供给 SDK。SDK 不导入 Flutter、Swift 或 Android 类型。
- Flutter 页面：继续调用原有钱包业务入口，处理需要展示的错误。
- 普通钱包 `wallet-tree::IoStrategy` 与 `KeystoreApi` 当前是同步接口，且包含直接文件操作。需要按单一流程拆分序列化/加密与 I/O，明确异步边界，不能直接在运行时线程里 block_on 原生回调。
- API 钱包的 phrase/seed 编码与数据库操作已在异步流程内，更适合下一批先验证一条字节保护链路。

## 后续批次

1. 原生适配器：选一个平台，用合成数据验证真实硬件密钥、重启加载、锁屏/缺失/错误传播。平台桥接尚未证明可用。
2. 一条 SDK 实际存储流程：覆盖写入、读取、失败不落旧格式、并发和原子替换。
3. 旧数据升级：新密文读回校验后再切换，崩溃恢复与回滚策略；不得自动覆盖失效密钥。
4. 覆盖另一平台及其余普通/API 钱包数据、password_proof 和备份副本。

尚未完成：真实 SEP/TEE、FFI 跨语言调用、进程重启、SQLite、迁移、原子替换、取消后的原生操作生命周期、设备恢复。四项软件测试不能作为审计 H2 关闭依据。


## 第二批：真实 Secure Enclave 验证

本机 Apple Silicon macOS 上以 Swift/CryptoKit 完成真实 SEP 验证。临时源码、可执行文件及合成数据均在 `target/device-protection-sep/`。没有读取实际钱包，也没有创建生产 Keychain 条目。

受限执行环境返回 SecureEnclave unavailable；经工具授权在沙箱外重跑后，SecureEnclave.isAvailable 为 true，以下验证通过：

1. SEP P-256 KeyAgreement 密钥参与 ECDH，HKDF-SHA256 派生 AES-256-GCM 密钥，合成数据往返成功。
2. 将 CryptoKit dataRepresentation（设备绑定的包裹表示，不是明文私钥）和密文保存到 target，退出后由另一个进程恢复并解密成功。
3. 变更用途 AAD、篡改密文、使用另一把真实 SEP 密钥均被拒绝。

```sh
xcrun swiftc -module-cache-path target/device-protection-sep/module-cache target/device-protection-sep/probe.swift -o target/device-protection-sep/probe
target/device-protection-sep/probe target/device-protection-sep write
target/device-protection-sep/probe target/device-protection-sep read
```

后两条需要本环境的沙箱外权限才能访问 SEP。测试未使用软件密钥替代 SEP 长期密钥；临时 ECDH 私钥、共享秘密、派生 AES 密钥和解密结果仍会存在应用内存。

边界：这是 macOS 硬件验证，不是 iPhone 实测；没有证明文件拷到另一台物理设备的行为，也没有验证 Keychain 保存、WhenUnlockedThisDeviceOnly ACL、生物认证、重启设备、换机恢复或插件 FFI。测试重启的是进程，不是操作系统。下一批需在插件接入单一 iOS 设备保护适配器，并验证实际 SDK 异步调用及 iPhone 行为。


## 正式实现第一批：SDK 接口

新增 `wallet_api::device_protection`：实例级 `Arc<dyn DeviceProtectionProvider>`、异步字节加解密、稳定记录身份/用途的 AAD 编码，以及不携带原始平台错误的类型化错误。默认无 provider 时拒绝操作，不返回输入作为降级。

5 项契约测试通过，包含 Pending 到 Ready 的延迟响应；代码审查无阻塞问题。断言矩阵见 `assertion-matrices/device-protection.md`。此批不包含 Context 初始化注入、插件注册或钱包读写接入；这些都仍是后续工作。

`cargo check -p wallet-api --offline` 通过（1m36s）；存在仓库原有警告，未扩大范围修复。

## Android 模拟器运行验证

2026-09-12：构建独立包 com.sevenwallet.protectionprobe（源码和 APK 在 target/android-device-protection/apk），在 emulator-5554 安装运行。未读取钱包数据或修改锁屏设置。测试结束已卸载独立测试 App。

- Android 运行时的合成数据 AES-GCM 往返、随机 IV、AAD、篡改、错密钥、格式和缺失密钥断言通过。
- 设备状态 secureLock=false、locked=false。
- 独立 AndroidKeyStore 能力探测 key 的 isInsideSecureHardware=false；探测 key 已删除。
- 正式适配器 provision 返回 UNAVAILABLE，未降级。由于首先被无安全锁屏检查拒绝，本次不构成其软件密钥等级拒绝分支的动态覆盖。
- 无连接的 Android 真机，因此 TEE/StrongBox 成功路径、锁屏变化、重启恢复仍待真机测试。未接通 Flutter/FRB。

## Android 真机 TEE 验证

2026-09-12，ANA-AN00，API 31：独立测试包运行通过。设备安全锁屏已开启且当前解锁；实际适配器报告 TEE（非 StrongBox）。真实 AndroidKeyStore AES-GCM 往返、持久化合成密文、显式 force-stop 后新进程解密，以及真实密钥下篡改/错误 AAD 拒绝全部通过。进程 PID 已变化，未重启手机。测试结束卸载独立测试包并清理测试密钥及合成数据，未接触钱包。

仍未验证：StrongBox、锁屏状态切换/移除锁屏、系统重启、另一台设备解密拒绝、Flutter/FRB 及实际 SDK 存储集成。本次证据证明该真机上原生 TEE 适配工作的成功路径，不代表完整钱包已启用设备保护。

## iOS 正式原生模块

新增插件 ios/Classes/IOSEnvelope.swift 与 IOSDeviceProtection.swift，明确 SEP + Keychain 存储、显式初始化、缺失不重建、版本化 AAD 绑定。审查修正了已有引用恢复失败的 keyMissing/keyInvalidated 分类，避免误入初始化分支。实际安装文件的 CryptoKit 封装测试通过；iPhoneOS arm64/iOS11 target 类型检查通过（实现受 iOS13 availability 保护）。测试说明位于插件 ios/docs/device-protection.md。

仍无 iPhone 真机运行结果，且双端均未完成 Flutter/FFI 注册及 SDK 敏感存储接入。不能将已完成原生模块等同于钱包数据已获得外层保护。

## Android MethodChannel 与 Dart 封装

插件新增 DeviceProtectionPlugin Android 注册、后台执行器、Dart DeviceProtectionChannel 和5项实际插件工程 messenger 测试。Java 使用 Android35/Flutter embedding 编译通过；测试全部通过。接口提供显式 provisionForNewStorage 和字节 protect/unprotect，SDK 版本未修改，尚无 Rust 注入/FRB 调用证据。

代码审查后将通道限制为4个未完成请求、单次约1 MiB，并在复制前拒绝超限请求；工作线程只捕获方法名而非完整 MethodCall。Dart 30秒超时仅停止等待，不保证原生操作取消；原生 detach 返回 CANCELLED、清空队列，已执行的 Keystore 操作可能完成。必须在后续真机 Flutter 测试覆盖队列压力与 detach。iOS 通道注册和 SDK 存储接入仍未完成。

## Rust/Dart 注入桥接完成（合成数据）

插件使用真实 SDK DeviceProtection trait，由实例化 DeviceProtectionBridge 注入 DartProtectionProvider；生成 FRB 绑定，Dart 转发器调用原生 MethodChannel。Rust/Dart 以固定错误类别传递失败，Rust35秒与Dart30秒等待超时不等同于取消原生操作。审查修正了加密前后大小上限差异，最大1MiB内层密文可带128字节封装开销读回。

实际插件4项Rust测试、7项Dart测试通过；Dart定向分析无问题；生成器、Android通道编译、测试profile动态库构建通过。另加载刚构建的真实Rust动态库，Flutter→Rust SDK trait→Dart callback往返及错误映射测试1项通过；其中MethodChannel使用测试替身，不能声称Android整机硬件调用链已验证。

尚未完成：iOS通道注册、Android Flutter真机整链路、Context/WalletManager注入和实际钱包读写切换。插件使用的SDK版本未在本批调整。

### iOS Flutter channel batch — 2026-09-13

Added plugin registration and a serial, bounded native channel (four pending requests). Byte-array/AAD/size checks, stable error codes, iOS 13 availability guard, and exactly-once cancellation on engine detach are implemented. Existing FFI registration remains enabled. Provisioning is explicit; no SDK storage routing changed.

Validation passed: iPhoneOS arm64/iOS 11 typecheck against installed real Flutter.framework; standalone Swift channel harness (async success, bad context, size bound, queue saturation, queued and running cancellation, post-detach rejection, missing-key error); CryptoKit envelope harness; seven Dart facade/callback tests. Review found non-byte typed-array acceptance and it was corrected to UInt8-only, then typechecked again. Flutter adapter runtime, iPhone Keychain/SEP, full CocoaPods build and Android Flutter full-chain testing remain pending.

### Android Flutter probe packaging and emulator — 2026-09-13

Built an isolated app under target/protection_fullchain with a snapshot of current wallet_plugin, synthetic config `{}`, and synthetic payload only. No WalletManager, real wallet storage or production config used. Debug APK: target/protection_fullchain/build/app/outputs/flutter-apk/app-debug.apk. Package com.sevenwallet.protection_fullchain, backups disabled.

Build succeeded (actual aarch64-linux-android Rust library, generated FRB, Flutter and native Android channel). Final Dart analyze passed. Environment adjustments are documented in the probe README: isolated Cargo workspace, cached Kotlin plugin mapping, installed NDK version/host path, local Maven cache and official Flutter dependencies. These are disposable probe changes, not edits to the production plugin build.

Emulator runtime: Rust library initialized; actual Rust provider -> Dart callback -> native Android channel -> Rust error returned UNAVAILABLE as expected. This checks real error propagation with no mocked messenger; it does not prove hardware success. The emulator lacks the required secure hardware/lock configuration. Native provisioning was unavailable and no synthetic ciphertext was created. Probe app uninstalled after testing.

Physical phone was not connected (adb listed only emulator-5554). Pending: install APK on an unlocked secure physical Android device, verify ALL PASS for persisted roundtrip/tamper/context tests, force-stop/relaunch and verify existing ciphertext reload without provisioning. iPhone runtime and real SDK storage routing remain pending.

### Android physical-device full chain — 2026-09-13

Installed the isolated Flutter probe APK on connected Huawei ANA-AN00. First run (PID 9569) initialized the real Rust library, provisioned a TEE-backed key, wrote synthetic ciphertext, and reported PASS persisted roundtrip, PASS tamper rejection, PASS context rejection, ALL PASS. This exercises Flutter -> actual Android Rust SDK provider -> Dart callback -> real Android MethodChannel -> Android hardware adapter, without mocked messenger or cryptography.

Force-stopped only the probe app and relaunched. New PID 9989 reported Reloading existing ciphertext without provisioning, then all three checks and ALL PASS again. This verifies process-restart persistence; no phone reboot, lock transition, StrongBox or different-device test was performed. No real wallet storage was read or migrated. Test app removed after completion; APK and source retained in target/protection_fullchain. SDK production storage integration and iPhone runtime remain pending.

### SDK create-only protected file component — 2026-09-13

Added wallet-api::device_protection::file_store::ProtectedFileStore. It asynchronously protects already password-encrypted bytes, then uses a private same-directory tempfile and no-clobber publication. Existing destinations are never replaced. Reads are bounded and reject unrecognized/legacy envelopes before hardware calls. WDS v1 wraps native envelope bytes; stable wallet/purpose context is supplied by caller. This component neither provisions keys nor handles password decryption.

Ten contract/component tests passed, including five new disk tests: create/reopen roundtrip and wrong context, existing-file preservation/provider failure/legacy rejection, oversized input, concurrent one-winner publication, and oversized disk read. Fixture provider validates orchestration, not cryptography; hardware implementation was tested separately.

This batch does not route WalletManager/KeystoreApi calls into the component. Ordinary wallet I/O is synchronous and requires a subsequent async orchestration integration. Migration, field encryption/password_proof, directory crash durability and update semantics remain pending. Cancellation after worker dispatch may still publish a file; caller must verify before retry. A successful hard-link-based publication may leave an encrypted temporary link if unlink fails.

### User correction: existing I/O only; keystore compatibility bytes

The unused ProtectedFileStore and its file-creation tests were removed. Its previous validation record above is historical and no longer describes a shipped component. Existing wallet paths, file creation and directory management remain authoritative.

KeystoreProtection now provides byte-level seal/open behavior. New writes accept existing password-encrypted JSON and produce WDK v1 plus the native envelope; nested wrapping is rejected. Open distinguishes current format from legacy with an explicit ReadPolicy supplied by the caller. DeviceRequired rejects legacy bytes; unknown versions and native failures never fall back. LegacyMigrationAllowed first runs the existing wallet_crypto password decrypt, then wraps and verifies the original password ciphertext. The returned optional replacement is present only for legacy data. Current-format reads never rewrap. Password/plaintext buffers owned by this helper are zeroized and errors do not include their contents.

Caller integration is still required: durable wallet migration state, existing-path atomic replacement and readback, update migration state only after durable success, and deletion of old password-only copies. This module performs no I/O or native key provisioning. It does not yet enable migration in WalletManager or Flutter. Legacy/imported KDF resource limits and compatibility scope are documented in the assertion matrix; beyond-budget input is rejected without modification.

Validation: 11 device_protection contract/compatibility tests passed, including six keystore format tests; review follow-up found no remaining issue in the new resource guard. Test log: target/keystore-format-tests-final.log.

### SDK initialization injection — 2026-09-13

WalletManager::new_with_device_protection now accepts a DeviceProtection instance and installs it before global Context publication and manager worker startup. Existing new delegates with the legacy/default configuration. Reusing the same provider Arc is permitted; replacing/enabling/disabling a provider after initialization is rejected. Context exposes the immutable instance for subsequent existing-I/O integration, with no runtime setter.

Validation: 12 device_protection tests passed (including provider identity regression); cargo check -p wallet-api --offline passed with existing warnings. Independent review found no actionable issue. Identity tests do not start backend/global Context. Plugin manager initialization and ordinary wallet I/O consumption are the next batches; no storage cutover in this batch.

### Plugin automatic manager injection — 2026-09-13

Public plugin ffi.dart/wallet_plugin.dart keep the existing getApiManager arguments. Android/iOS automatically cache a native bridge and call the new protected Rust initializer; desktop/web call the legacy initializer. SDK callback instance is retained across manager retries, while failed bridge creation can retry. No App business-code edits were needed for its current public import. No provisioning or file migration added here.

Generated interface uses a required shared bridge reference; optional borrowed opaque input was not supported correctly by FRB2.12. Final generated Rust takes a read guard and Dart does not move the bridge. Eight Dart facade/channel tests passed, final five-item analysis passed, Rust test-profile dylib build passed, and actual FFI test passed (two invalid-path initialization attempts then same bridge protect/unprotect). Native messenger is mocked in this host FFI regression. Review passed.

Release limitation: plugin beta.256 pins local SDK 0434cc65ae981de61d5765f6d36f53aa9a46d411. Fresh remote fetch from the actual dependency server failed with not-our-ref. Earlier fetch from a working checkout was inconclusive due to existing objects. Commit was loaded from local SDK into Cargo cache solely for validation. No push was performed; publish the SDK commit to that server before distributing/building plugin elsewhere. Actual wallet I/O routing and migration state still pending.
