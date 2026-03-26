# NLA / CredSSP 인증 구현 계획

## 현재 상태

| 항목 | 현 상태 |
|------|---------|
| 보안 프로토콜 | Standard RDP Security (TLS-only) → **NLA/CredSSP 활성화 완료** |
| `enable_credssp` | `true` ([rdp.rs](../src/connection/rdp.rs) `build_config()`) ✅ |
| `sspi` 크레이트 | 이미 간접 의존 (ironrdp-connector → sspi 0.18.7, `scard` feature 포함) |
| CredSSP 핸들링 코드 | ironrdp-async `connect_finalize()` 내부에 **완전 구현되어 있음** |
| 도메인 필드 | `domain: None` 하드코딩 |
| TLS 인증서 검증 | `NoCertificateVerification` (비활성) |

### 핵심 발견

ironrdp-async 0.8.0의 `connect_finalize()`는 이미 다음 흐름을 포함:
```
connector.should_perform_credssp() → perform_credssp_step() → CredsspSequence::init() → NTLM/Kerberos 핸드셰이크 → connector.mark_credssp_as_done()
```

즉, IronRDP가 CredSSP/NLA 전체 프로토콜을 이미 구현하고 있으므로 **kterm이 설정만 올바르게 전달하면 동작**한다.

---

## 구현 계획

### Phase 1: 기본 NLA/CredSSP 활성화 (최소 변경) ✅ 적용 완료

**난이도**: 낮음  
**예상 변경 파일**: `src/connection/rdp.rs`

#### 1-1. `build_config()` — `enable_credssp: true`

```rust
// Before
enable_credssp: false,

// After
enable_credssp: true,
```

이 한 줄의 변경으로 ironrdp-connector의 `ClientConnector`가 X224 협상 시 **PROTOCOL_HYBRID (CredSSP)** 를 요청하게 된다. 서버가 CredSSP를 선택하면 `connect_finalize` 내부에서 자동으로 CredSSP 핸드셰이크를 수행한다.

#### 1-2. 프로토콜 협상 폴백 확인

`enable_credssp: true`와 `enable_tls: true`가 동시 설정되면 ironrdp-connector가 **PROTOCOL_HYBRID | PROTOCOL_SSL** 을 요청한다.
- 서버가 CredSSP를 지원하면 → NLA 수행
- 서버가 TLS만 지원하면 → TLS 폴백
- 서버가 둘 다 거부하면 → 에러

기존 TLS-only 서버와의 호환성이 유지되므로 **breaking change 없음**.

#### 1-3. 에러 메시지 세분화

CredSSP 실패 시 발생하는 에러 유형을 구분:

| 에러 | 사유 | 사용자 메시지 |
|------|------|-------------|
| `ConnectorErrorKind::Credssp(CredsspError)` | NTLM 인증 실패 (잘못된 자격증명) | "인증 실패: 사용자 이름 또는 비밀번호가 올바르지 않습니다" |
| `ConnectorErrorKind::Credssp(CredsspError)` 내부 `StatusCode::SEC_E_LOGON_DENIED` | 계정 잠김/만료 | "계정이 잠겨 있거나 만료되었습니다" |
| TLS handshake 실패 | 서버 인증서 문제 | 기존 에러 유지 |

```rust
// connect_finalize 에러 처리 개선
.map_err(|e| {
    match e.kind() {
        ConnectorErrorKind::Credssp(_) => format!("NLA 인증 실패: {:?}", e),
        _ => format!("connect_finalize failed: {:?}", e),
    }
})
```

#### 1-4. 검증 대상 환경

| 대상 | NLA 설정 | 예상 동작 |
|------|---------|----------|
| Windows 10/11 RDP 호스트 (기본 설정) | NLA 필수 (기본값) | CredSSP 성공 |
| Windows Server 2019+ | NLA 필수 (기본값) | CredSSP 성공 |
| XRDP (Linux) | NLA 비활성 (기본값) | TLS 폴백 |
| XRDP + NLA 활성 | NLA 활성 | CredSSP 성공 (XRDP ≥ 0.9.x) |
| Windows RDP (NLA 비활성) | TLS-only | TLS 폴백 |

---

### Phase 2: 도메인 인증 지원

**난이도**: 낮음  
**예상 변경 파일**: `src/connection/rdp.rs`, `src/main.rs`

#### 2-1. UI에 도메인 필드 추가

현재 RDP 연결 UI의 사용자명/비밀번호 필드 옆에 **도메인(Domain)** 입력 필드 추가.

- 사용자가 `DOMAIN\username` 또는 `username@domain.com` 형식으로 입력 시 파싱하거나, 별도 필드 사용
- `build_config()`에서 `domain` 파라미터를 UI 입력값으로 전달

```rust
// Before
domain: None,

// After  
domain: if domain_str.is_empty() { None } else { Some(domain_str) },
```

#### 2-2. UPN (User Principal Name) 파싱

사용자명에 `@` 포함 시 UPN으로 처리:
- `user@corp.local` → username: `user`, domain: `corp.local`
- `CORP\user` → username: `user`, domain: `CORP`

---

### Phase 3: TLS 서버 인증서 검증

**난이도**: 중간  
**예상 변경 파일**: `src/connection/rdp.rs`, `Cargo.toml`

#### 3-1. 현재 문제

`ironrdp-tls`는 내부적으로 `NoCertificateVerification`을 사용하여 **모든 서버 인증서를 무조건 수락**한다. CredSSP/NLA를 활성화해도 MITM 공격에 취약한 상태가 유지됨.

#### 3-2. 인증서 검증 구현 옵션

| 옵션 | 설명 | 복잡도 |
|------|------|--------|
| A. `ironrdp-tls` 커스터마이즈 | `ironrdp-tls`를 직접 사용하지 않고 `tokio-rustls` + `rustls-native-certs`로 직접 TLS 업그레이드 수행 | 중간 |
| B. 사용자 확인 팝업 | 최초 연결 시 서버 인증서 fingerprint를 표시하고 사용자에게 수락 여부를 확인 (SSH known_hosts 패턴) | 중간 |
| C. TOFU (Trust On First Use) | 최초 수락 후 로컬에 fingerprint 저장, 이후 변경 시 경고 | 중간~높음 |

**권장: B → C 순차 구현**

#### 3-3. 커스텀 TLS 업그레이드 (옵션 A 대체)

```rust
// ironrdp-tls::upgrade 대신 직접 TLS 핸드셰이크
use tokio_rustls::TlsConnector;
use rustls::{ClientConfig, RootCertStore};

let mut root_store = RootCertStore::empty();
// 시스템 인증서 저장소 로드
for cert in rustls_native_certs::load_native_certs()? {
    root_store.add(cert)?;
}

let config = ClientConfig::builder()
    .with_root_certificates(root_store)
    .with_no_client_auth();

let connector = TlsConnector::from(Arc::new(config));
let tls_stream = connector.connect(server_name, tcp_stream).await?;
```

**주의**: RDP 서버는 대부분 자체 서명 인증서를 사용하므로, 시스템 CA만으로는 검증 실패할 가능성 높음. TOFU 패턴이 현실적.

---

### Phase 4: Kerberos 인증 지원 (선택적)

**난이도**: 높음  
**예상 변경 파일**: `src/connection/rdp.rs`, `Cargo.toml`

#### 4-1. 배경

NLA/CredSSP는 내부적으로 SSPI를 통해 인증을 수행하며, 두 가지 메커니즘을 지원:
- **NTLM**: 사용자명/비밀번호 기반 (기본, Phase 1에서 자동 사용)
- **Kerberos**: 도메인 환경에서의 토큰 기반 인증

#### 4-2. Kerberos 활성화 조건

`connect_finalize()`의 마지막 파라미터 `kerberos_config: Option<KerberosConfig>`:

```rust
pub struct KerberosConfig {
    pub kdc_proxy_url: url::Url,  // KDC Proxy (KKDCPMessage over HTTPS)
}
```

- 도메인 참가 환경에서 KDC Proxy URL을 설정하면 Kerberos 인증 수행
- 현재는 `None` 전달 중 → NTLM 폴백 동작

#### 4-3. 구현 범위

| 항목 | 설명 |
|------|------|
| KDC Proxy URL 설정 필드 | 고급 RDP 설정에 추가 |
| `sspi` `network_client` feature | Kerberos KDC proxy용 reqwest 클라이언트 (이미 `ReqwestNetworkClient`로 전달 중) |
| Kerberos 자동 감지 | 도메인이 설정되어 있으면 KDC Proxy 자동 탐색 시도 (복잡) |

**우선순위**: 낮음. 대부분의 사용 시나리오에서 NTLM으로 충분.

---

## 구현 순서 및 우선순위

```
Phase 1 ─── enable_credssp: true + 에러 세분화
  │          [최소 변경, 즉시 적용 가능]
  │
Phase 2 ─── 도메인 필드 UI + UPN 파싱  
  │          [Phase 1 직후]
  │
Phase 3 ─── TLS 인증서 검증 (TOFU)
  │          [보안 강화, 중기 목표]
  │
Phase 4 ─── Kerberos 인증
             [장기 목표, 엔터프라이즈 환경]
```

---

## Phase 1 구체적 변경 사항

### 파일: `src/connection/rdp.rs`

#### 변경 1: `build_config()` 함수

```diff
 fn build_config(username: String, password: String, domain: Option<String>) -> connector::Config {
     connector::Config {
         credentials: Credentials::UsernamePassword { username, password },
         domain,
         enable_tls: true,
-        enable_credssp: false,
+        enable_credssp: true,
         keyboard_type: KeyboardType::IbmEnhanced,
```

#### 변경 2: `connect()` 함수 에러 핸들링

```diff
     let connection_result = connect_finalize(
         upgraded,
         connector,
         &mut upgraded_framed,
         &mut network_client,
         server_name.into(),
         server_public_key,
         None, // kerberos_config — Phase 4
     )
     .await
-    .map_err(|e| format!("connect_finalize failed: {:?}", e))?;
+    .map_err(|e| {
+        if format!("{:?}", e).contains("redssp") || format!("{:?}", e).contains("Credssp") {
+            format!("NLA 인증 실패 — 사용자 이름 또는 비밀번호를 확인하세요: {}", e)
+        } else {
+            format!("RDP 연결 실패: {:?}", e)
+        }
+    })?;
```

### 리스크 및 완화

| 리스크 | 영향 | 완화 |
|--------|------|------|
| CredSSP 미지원 서버에서 연결 실패 | `enable_tls: true` 동시 설정으로 자동 폴백 | 테스트 확인 필요 |
| NTLM 인증이 서버 정책으로 차단 | Kerberos 미지원이므로 실패 | Phase 4에서 대응 |
| `sspi` 크레이트 호환성 | v0.18.7 이미 의존 트리에 포함 | 추가 의존성 없음 |
| 빌드 시간 증가 | `sspi`는 이미 컴파일 중 | 변동 없음 |
| TLS 인증서 미검증 상태에서의 MITM | NLA 사용해도 MITM 가능 | Phase 3에서 대응 |

---

## 요약

**Phase 1은 `enable_credssp: false` → `true` 변경 하나로 NLA를 활성화**할 수 있다. ironrdp-async/ironrdp-connector가 CredSSP 핸드셰이크(NTLM) 전체를 이미 구현하고 있고, `sspi` 크레이트도 이미 의존 트리에 포함되어 있어 추가 의존성이 필요 없다. 핵심은 에러 핸들링 세분화와 다양한 서버 환경에서의 폴백 검증이다.
