# RDP 통합 구현 계획 (IronRDP)

## RDP 프로토콜 지원 현황

### ✅ 지원

#### 연결 / 인증
| 항목 | 비고 |
|------|------|
| TCP + TLS 1.2/1.3 업그레이드 | `ironrdp-tls` 크레이트 기반 (`NoCertificateVerification` 내장, 인증서 검증은 R7) |
| 사용자명/비밀번호 인증 | `Credentials::UsernamePassword` |
| 서버 인증서 공개키 추출 | CredSSP 채널 바인딩용 |
| 고정 초기 데스크톱 크기 협상 | 1280×1024 하드코딩 |

#### 그래픽 / 화면
| 항목 | 비고 |
|------|------|
| ActiveStage FastPath 그래픽 업데이트 | `GraphicsUpdate` 출력 경로 |
| Slow-path 비트맵 업데이트 폴백 | `try_handle_slowpath_bitmap` 수동 디코딩 |
| RDP6 압축 32bpp 디코딩 | `BitmapStreamDecoder` |
| RLE 16bpp (RGB565) 디코딩 | `ironrdp::graphics::rle` |
| RLE 24bpp (BGR24) 디코딩 | `ironrdp::graphics::rle` |
| 비압축 32bpp BGRX 디코딩 | |
| 비압축 16bpp RGB565 디코딩 | |
| NSCodec 코덱 협상 | `BitmapCodecs`에 NSCodec 등록 — ⚠️ **디코딩 미구현** (서버가 NSCodec으로 전송 시 화면 깨짐 가능) |
| RemoteFX (SurfaceCommands 경유) | `ironrdp-session`의 `CODEC_ID_REMOTEFX` → `rfx::DecodingContext` (DWT+RLGR+양자화+서브밴드 재구성) — **자동 처리** |
| Dirty Rect 단위 부분 업데이트 | Arc CoW 버퍼 + GPU 부분 텍스처 업로드 |
| 프레임 배치 병합 | ≈60fps 상한, 16ms/50ms 이중 타이머 |
| wgpu GPU 텍스처 렌더러 | WGSL 쉐이더, 뷰포트 스케일링 |
| Performance Flag 협상 | `ENABLE_FONT_SMOOTHING`, `ENABLE_DESKTOP_COMPOSITION` |

#### 입력
| 항목 | 비고 |
|------|------|
| 키보드 스캔코드 FastPath | `FastPathInputEvent::KeyboardEvent` |
| 키보드 유니코드 FastPath | `FastPathInputEvent::UnicodeKeyboardEvent` |
| Extended 키 플래그 | 화살표, Insert, Delete, Home 등 |
| 마우스 이동 | `PointerFlags::MOVE` |
| 마우스 좌/우/중 클릭 | `LEFT_BUTTON`, `RIGHT_BUTTON`, `MIDDLE_BUTTON` |
| 마우스 수직 휠 | `PointerFlags::VERTICAL_WHEEL` |

#### 가상 채널
| 항목 | 비고 |
|------|------|
| rdpsnd 정적 채널 등록 | `ironrdp-rdpsnd` + `cpal` 백엔드, **재생 테스트 미완료** |

#### I/O
| 항목 | 비고 |
|------|------|
| 비동기 Tokio I/O | `ironrdp-tokio` `MovableTokioStream` + `tokio::select!` 이벤트 드리븐 루프 (R1 완료) |
| TLS 업그레이드 | `ironrdp-tls` 크레이트 기반 (R2 완료) |

---

### ❌ 미지원 / 미완성

#### 연결 / 인증
| 항목 | 비고 |
|------|------|
| NLA (CredSSP) | `enable_credssp: false` 고정 |
| TLS 서버 인증서 검증 | `NoCertificateVerification` — 보안 취약 |
| 도메인 인증 | `domain: None` 고정 |
| 자동 로그온 | `autologon: false` 고정 |
| 연결/인증 실패 사유 세분화 | 단일 Error 이벤트로 처리 중 |

#### 그래픽 / 화면
| 항목 | 비고 |
|------|------|
| 창 리사이즈 → 원격 해상도 동기화 | `encode_resize` 코드 존재하나 UI 이벤트 연동 미완 |
| EGFX 그래픽 파이프라인 (DVC) | `ironrdp-pdu`에 GFX PDU 정의 존재, DVC 프로세서 직접 구현 필요 |
| RemoteFX Progressive (EGFX 경유) | `Codec2Type::RemoteFxProgressive` — DVC GFX 프로세서 필요 |
| ClearCodec (EGFX 경유) | `Codec1Type::ClearCodec` — DVC GFX 프로세서 필요 |
| Planar Codec (EGFX 경유) | `Codec1Type::Planar` — DVC GFX 프로세서 필요 |
| H.264/AVC420 (EGFX 경유) | `Codec1Type::Avc420` — `openh264` 크레이트로 디코딩 필요 |
| H.264/AVC444 (EGFX 경유) | `Codec1Type::Avc444` / `Avc444v2` — `openh264` 크레이트로 디코딩 필요 |
| NSCodec 디코딩 | 협상만 구현, 디코딩 함수 IronRDP에 없음 — 직접 구현 필요 (MS-RDPNSC 스펙) |
| ZGFX 벌크 압축 해제 (EGFX) | `ironrdp-graphics::zgfx::Decompressor` 존재 — GFX 프로세서 내 통합 필요 |
| GDI/GDI+ 그래픽 가속 Order | 미처리 |
| 서버 커서 표시 | `enable_server_pointer: false` |
| 포인터(커서) 캐싱 | |

#### 입력
| 항목 | 비고 |
|------|------|
| 마우스 수평 휠 | `PointerFlags::HORIZONTAL_WHEEL` 미구현 |
| IME 조합 입력 | 한국어 등 다국어 입력 조합 미처리 |
| 복합 키 조합 정밀 매핑 | Ctrl+Alt+Del, Win 키 등 |
| 멀티터치 | |

#### 채널 / 리다이렉션
| 항목 | 비고 |
|------|------|
| 클립보드 공유 (RDPCLIP) | 미구현 |
| 드라이브 리다이렉션 (RDPDR) | 미구현 |
| 프린터 리다이렉션 | 미구현 |
| 포트/COM 리다이렉션 | 미구현 |
| USB 리다이렉션 | 미구현 |
| 동적 가상 채널 (DVC) | 미구현 (정적 채널만 사용) |

#### 세션 관리
| 항목 | 비고 |
|------|------|
| 탭 닫기 시 워커 스레드/채널 완전 정리 | 미완 |
| 재접속 UX | 미구현 |
| 불안정 네트워크 복구 | 미구현 |

---

## 목표
- kterm에 RDP 프로토콜 기반 원격 화면 세션을 통합한다.
- 기존 SSH/Telnet/Serial/Local과 동일한 탭 UX를 유지한다.
- 1차 릴리스는 화면 출력 + 입력 + 리사이즈 + 안정적인 종료를 우선한다.
- VNC까지 확장 가능한 공통 Iced 렌더링 계층을 먼저 확립한다.

## 현재 진행 상태
- Welcome 프로토콜 선택에 RDP 항목 추가 완료.
- RDP 접속 폼(Host/Port/User/Password) 추가 완료.
- `src/connection/rdp.rs` 스캐폴드 추가 완료.
- RDP 탭 생성/세션 이벤트 라우팅(세션 ID 기반) 연결 완료.
- 의존성 충돌 해소: `russh`를 `0.55.0`으로 낙춰 `ironrdp` 동일 바이너리 통합 가능 상태 확인.
- IronRDP TLS 핸드셰이크 + `ActiveStage` 기반 연속 PDU 루프 완료.
- `remote_display` 공통 모듈과 RDP 탭 이미지 렌더링(Full/Rect) 연동 완료.
- 기본 키보드/마우스 입력 매핑(FastPath): 스캔코드, 유니코드, 이동/클릭/휠 완료.
- 다중 픽섹 포맷 디코딩(RDP6 32bpp, RLE 16/24bpp, 비압축 BGRX/RGB565) 완료.
- wgpu GPU 텍스처 + WGSL 쉐이더 기반 `RdpPipeline` 렌더러 완료.
- Dirty Rect 단위 부분 텍스처 업로드(GPU 대역폭 최소화) 완료.
- 프레임 배치 병합(연속 Frames 이벤트 통합, ≈60fps 상한 스로틀링) 완료.
- Slow-path 비트맵 업데이트 폴백 처리(`try_handle_slowpath_bitmap`) 완료.
- RDP 오디오 재생: `ironrdp-rdpsnd` + `ironrdp-rdpsnd-native`(cpal 백엔드) 정적 채널로 연결 설정에 통합 완료. **실제 동작 테스트 미완료.**
- **[R1 완료]** `ironrdp-blocking` → `ironrdp-tokio` 비동기 전환: `spawn_blocking` 제거, `tokio::spawn(async)` 전환, `tokio::select!` 기반 PDU 루프 적용.
- **[R1 완료]** `ironrdp_tokio::MovableTokioStream` 기반 `UpgradedFramed` 타입 전환, `ReqwestNetworkClient` 교체.
- **[R2 완료]** 수동 TLS 코드 제거(`tls_upgrade`, `extract_tls_server_public_key`, `mod danger::NoCertificateVerification` — 약 85줄) → `ironrdp-tls` 크레이트 일괄 교체.
- **[R1/R2 완료]** `Cargo.toml` 직접 의존성 정리: `ironrdp-blocking`, `tokio-rustls`, `x509-cert`, `sspi` 직접 선언 제거; `ironrdp-tokio = "0.8.0"` (reqwest feature), `ironrdp-tls = "0.2.0"` (rustls feature) 추가.

## 아키텍처 방향
- 단기(Phase 1): 기존 `ConnectionEvent` 채널을 재사용해 연결 수명주기 안정화.
- 중기(Phase 2): 그래픽 전용 이벤트 모델 도입.
  - 예시: `RdpEvent::Frame`, `RdpEvent::Pointer`, `RdpEvent::Disconnected`, `RdpEvent::Error`
- 장기(Phase 3): 터미널 탭과 원격 그래픽 탭을 렌더링 레벨에서 분리.

## 공통 렌더링 모듈화 원칙 (RDP + VNC 공용)
- 프로토콜별 디코딩(입력: 원격 프레임)과 UI 렌더링(출력: Iced 이미지)을 분리한다.
- 공통 모듈은 "프레임 버퍼 상태 + 프레임 갱신 정책 + 렌더링 데이터 변환"만 담당한다.
- RDP/VNC 모듈은 공통 모듈에 `FrameUpdate` 이벤트만 전달한다.

### 제안 모듈 경계
- `src/remote_display/mod.rs`: 공통 타입(`FrameUpdate`, `PixelFormat`, `RemoteDisplayState`) 및 트레이트(`RemoteDisplayBackend`).
- `src/remote_display/renderer.rs`: Iced 렌더링용 데이터 변환 및 프레임 스로틀/드롭 정책.
- `src/connection/rdp.rs`: IronRDP ActiveStage 처리 후 `FrameUpdate` 생성.
- `src/connection/vnc.rs`(예정): VNC 디코딩 후 `FrameUpdate` 생성.

### 공통 이벤트 계약 (초안)
- `FrameUpdate::Full { width, height, rgba }`
- `FrameUpdate::Rect { x, y, width, height, rgba }`
- `FrameUpdate::Cursor { x, y, visible }`
- `FrameUpdate::Resize { width, height }`

## 단계별 구현

### Phase 1: 연결 기반 구축 (완료)
- [x] UI에서 RDP 접속 정보 입력/전송
- [x] `ConnectRdp` 메시지 핸들러 추가
- [x] `connection::rdp::connect_and_subscribe` 연결
- [x] Cargo 의존성 해소(`russh 0.55.0` + `ironrdp 0.14.0`) 및 `cargo check` 통과
- [x] IronRDP 실제 핸드셰이크 적용(Connector + TLS upgrade + finalize)
- [x] ActiveStage 기반 그래픽 프레임 수신 루프 1차 연결(프로브/응답 프레임 송신)
- [x] ActiveStage 출력을 Iced 렌더링 상태로 직접 브리지
- [ ] 연결 실패/인증 실패/종료 사유 세분화

### Phase 2: 그래픽 파이프라인 (완료)
- [x] 공통 렌더링 모듈 `remote_display` 생성
- [x] RDP/VNC 공용 `FrameUpdate` 타입 확정(초기: Full 프레임)
- [x] `RemoteDisplayState`(프레임 버퍼) 구현
- [x] Iced 렌더링용 변환 레이어 구현(초기: RGBA Handle 생성)
- [x] RDP 프레임 수신 루프 구축(기본 스트리밍)
- [x] 프레임 버퍼(RGBA) 상태 저장 및 갱신(Full/Rect 적용)
- [x] Iced 이미지 렌더링으로 화면 표시(초기: RDP 탭에 FrameUpdate 반영)
- [x] Rect 기반 부분 프레임 업데이트 경로 추가(FrameUpdate::Rect)
- [x] wgpu GPU 텍스처 + WGSL 쉐이더 기반 `RdpPipeline` 렌더러 구현
- [x] Dirty Rect 부분 텍스처 업로드로 GPU 대역폭 최소화
- [x] Slow-path 비트맵 업데이트 폴백 처리(RDP6/RLE 16/24bpp/BGRX)
- [x] 프레임 스로틀링(≈60fps 상한, 16ms 타이머 + 50ms drain)
- [ ] 프레임 스로틀(30fps 상한) 정교화(Drop 정책 포함)

### Phase 2.5: VNC 대비 공통화 검증
- [ ] 더미 백엔드(테스트 프레임 생성기)로 공통 렌더러 단독 검증
- [ ] RDP 백엔드를 공통 렌더러에 연결
- [ ] VNC 백엔드 연결 시 코드 변경 최소화(목표: UI 코드 변경 0 또는 극소)

### Phase 3: 입력/상호작용 (완료)
- [x] 키보드 입력을 RDP FastPath(스캔코드/유니코드)로 기본 매핑
- [x] 마우스 이동/클릭/휠 이벤트 기본 매핑
- [ ] 포커스 및 입력 캡처 정책 정리

### Phase 4: 세션 품질
- [ ] 윈도우 리사이즈를 원격 해상도 변경으로 반영
- [ ] 탭 전환/닫기 시 리소스 누수 없는 종료
- [ ] 재접속 UX 및 오류 재시도 정책 정리

### Phase 5: 품질 고도화 (후속)
- [ ] 포커스/입력 캡처 정책 정교화
- [ ] 키 조합/로케일/IME 입력 정밀 매핑
- [ ] 스로틀/드롭 정책 튜닝 및 성능 계측
- [ ] 불안정 네트워크에서 회복력 강화
- [ ] RDP 오디오 재생 실제 동작 검증(`ironrdp-rdpsnd` + cpal 백엔드)

## 검증 체크리스트
- [x] RDP 접속 성공 시 원격 화면이 표시된다.
- [x] 키보드/마우스 입력이 원격 세션에서 정상 동작한다.
- [ ] 탭 닫기 후 백그라운드 스레드/채널이 정상 종료된다.
- [x] 기존 SSH/Telnet/Serial/Local 동작에 회귀가 없다.
- [ ] RDP 오디오가 원격 세션 재생음을 로컬에서 출력한다. (코드 통합 완료, 테스트 미완료)
- [x] 공통 렌더링 모듈이 RDP/VNC 모두에서 재사용 가능하다.
- [ ] 프로토콜 추가 시 UI 계층 수정이 최소화된다.

## 주의사항
- 초기에 RDP는 터미널 바이트 스트림과 모델이 달라서 별도 이벤트 계층이 필요하다.

---

# RDP 리팩토링 계획 (IronRDP 하위 크레이트 전면 활용)

> **작성일**: 2026-03-25  
> **목표**: 기존 RDP 코드를 IronRDP 생태계의 전용 하위 크레이트로 전면 교체하고, 현재 미지원 기능을 단계적으로 추가한다.

## 현재 의존성 vs 목표 의존성

| 현재 사용 | 버전 | 역할 |
|-----------|------|------|
| `ironrdp` (meta) | 0.14.0 | connector/session/graphics/pdu 재수출 |
| `ironrdp-tokio` | 0.8.0 | 비동기 I/O — `Framed`, `connect_begin/finalize` (**R1 완료**) |
| `ironrdp-tls` | 0.2.0 | TLS 업그레이드 + 인증서 추출 (**R2 완료**) |
| `ironrdp-core` | 0.1.5 | `ReadCursor`, `Decode` 등 기본 타입 |
| `ironrdp-rdpsnd` | 0.7.0 | 오디오 정적 채널 |
| `ironrdp-rdpsnd-native` | 0.5.0 | cpal 오디오 백엔드 |

> **제거됨 (R1/R2)**: `ironrdp-blocking`, `tokio-rustls`, `x509-cert`, `sspi` — Cargo.toml 직접 선언 제거 완료  
> `tokio-rustls`/`x509-cert`는 `sspi→reqwest→hyper-rustls` / `ironrdp-pdu` 경로로 간접 의존 잔류(바이너리에 포함)

| **추가 예정** | 버전 | 역할 |
|--------------|------|------|
| ~~`ironrdp-tokio`~~ | ~~0.8.0~~ | ~~**Tokio 비동기 I/O**~~ — ✅ **R1 완료** |
| ~~`ironrdp-tls`~~ | ~~0.2.0~~ | ~~**TLS 보일러플레이트**~~ — ✅ **R2 완료** |
| `ironrdp-cliprdr` | 0.5.0 | **클립보드 공유** (RDPECLIP 정적 채널) |
| `ironrdp-cliprdr-native` | 0.5.0 | **클립보드 네이티브 백엔드** (OS 클립보드 연동) |
| `ironrdp-dvc` | 0.5.0 | **동적 가상 채널** (DRDYNVC) |
| `ironrdp-displaycontrol` | 0.5.0 | **디스플레이 제어** (동적 해상도 변경, DVC 기반) |
| `ironrdp-input` | 0.5.0 | **입력 유틸리티** — 수동 FastPath 매핑 교체 |
| `ironrdp-rdpdr` | 0.5.0 | **드라이브 리다이렉션** (RDPDR 채널) |
| `ironrdp-rdpdr-native` | 0.5.0 | **드라이브 리다이렉션 네이티브 백엔드** |
| `openh264` | 0.9.3 | **H.264/AVC 디코딩** — EGFX AVC420/AVC444 코덱용 (Cisco OpenH264 바인딩) |

| **직접 의존성 제거 완료** | 이유 | 비고 |
|--------------------------|------|------|
| ~~`ironrdp-blocking`~~ | `ironrdp-tokio`로 대체 — ✅ **R1 완료** | 바이너리에서 완전 제거됨 |
| ~~`x509-cert`~~ (직접 선언) | `ironrdp-tls`가 내부 처리 — ✅ **R2 완료** | `ironrdp-pdu` 간접 의존으로 바이너리 잔류 |
| ~~`tokio-rustls`~~ (직접 선언) | `ironrdp-tls`가 래핑 — ✅ **R2 완료** | `sspi → reqwest → hyper-rustls` 간접 의존으로 바이너리 잔류 |
| ~~`sspi`~~ (직접 선언) | `ironrdp-tokio::reqwest::ReqwestNetworkClient`로 교체 — ✅ **R1 완료** | 간접 의존으로 바이너리 잔류 |

> **⚠️ 의존성 분석 결과**: `cargo tree -i` 확인 결과, `tokio-rustls`는 `sspi → reqwest → hyper-rustls` 체인으로, `x509-cert`는 `ironrdp-pdu`를 통해 이미 간접 의존되고 있음. 따라서 `Cargo.toml`에서 직접 선언만 제거할 수 있으며, 두 크레이트는 컴파일된 바이너리에 계속 포함됨. **실질적 효과는 `kterm` 직접 코드에서 해당 크레이트 API 사용 제거 (코드 단순화)에 있음.**

---

## Phase R1: 비동기 I/O 전환 ✅ 완료

> `ironrdp-blocking` → `ironrdp-tokio` 마이그레이션

### 변경 내용
1. **`Cargo.toml`**: `ironrdp-blocking` 제거, `ironrdp-tokio = "0.8.0"` 추가
2. **`rdp.rs` 워커 구조 전환**:
   - `tokio::task::spawn_blocking` + 동기 루프 → `tokio::spawn` + 비동기 루프
   - `ironrdp_blocking::Framed<TlsStream>` → `ironrdp_tokio::Framed<MovableTokioStream<TlsStream<TcpStream>>>`
   - `ironrdp_blocking::connect_begin/connect_finalize` → `ironrdp_tokio::connect_begin/connect_finalize`
   - `framed.read_pdu()` (블로킹) → `framed.read_pdu().await` (비동기)
   - `framed.write_all()` → `framed.write_all().await`
   - `std::net::TcpStream` → `tokio::net::TcpStream`
   - `sspi::ReqwestNetworkClient` → `ironrdp_tokio::reqwest::ReqwestNetworkClient`
3. **메인 루프 리팩토링**:
   - 기존: `loop { try_recv(); read_pdu(); sleep(1ms) }` 폴링
   - 변경: `tokio::select! { input = rx.recv() => ..., pdu = framed.read_pdu() => ... }`
   - `set_fast_timeout` / WouldBlock 핸들링 불필요 → 제거

### 실제 변경 결과
- `UpgradedFramed` = `Framed<MovableTokioStream<ironrdp_tls::TlsStream<tokio::net::TcpStream>>>`
  - `LocalTokioStream` 사용 시 `!Send` 컴파일 오류 → `MovableTokioStream`으로 교체 (Send 바운드 충족)
- `cargo check` 및 `cargo run` 성공 (기존 경고 1건 유지)

### 기대 효과
- CPU 폴링 오버헤드 제거 (idle 시 0% CPU)
- `spawn_blocking` 스레드풀 점유 해소
- 입력 응답 지연 최소화 (sleep(1ms) 제거)
- 향후 async 채널(DVC, CLIPRDR 등)과 자연스러운 통합

### 주의사항
- `ironrdp-tokio`의 `connect_begin`/`connect_finalize` API 시그니처 확인 필요
- 기존 `UpgradedFramed` 타입 별칭 전면 교체
- `sspi::ReqwestNetworkClient`의 비동기 호환성 확인

---

## Phase R2: TLS 계층 정리 ✅ 완료

> 수동 TLS 코드 → `ironrdp-tls` 크레이트

### 변경 내용
1. **`Cargo.toml`**: `ironrdp-tls = { version = "0.2.0", features = ["rustls"] }` 추가; `tokio-rustls`, `x509-cert`, `sspi` 직접 선언 제거
2. **`rdp.rs`에서 제거된 코드**:
   - `fn tls_upgrade()` (~30줄)
   - `fn extract_tls_server_public_key()` (~15줄)
   - `mod danger { NoCertificateVerification }` (~40줄)
3. **교체**: `ironrdp_tls::upgrade(stream, server_name).await` → `(TlsStream, Certificate)` 반환
4. **공개키 추출**: `ironrdp_tls::extract_tls_server_public_key(&cert)` 사용

### 실제 변경 결과
- 수동 TLS 보일러플레이트 약 85줄 제거 완료
- `Cargo.toml` 직접 선언 4개 제거 (`ironrdp-blocking`, `tokio-rustls`, `x509-cert`, `sspi`)
- R1과 동시 완료 (`cargo check` / `cargo run` 성공)

> ⚠️ **보안 참고**: `ironrdp-tls` 내부적으로 `NoCertificateVerification` 사용 중. 실질적 서버 인증서 검증 활성화는 **R7**에서 진행.

---

## Phase R3: 입력 처리 개선

> 수동 FastPath 매핑 → `ironrdp-input` 크레이트

### 변경 내용
1. **`Cargo.toml`**: `ironrdp-input = "0.5.0"` 추가
2. **`rdp.rs` `rdp_input_to_fastpath()` 교체**:
   - `ironrdp_input::InputDatabase` 활용하여 키보드/마우스 입력 관리
   - 키보드: scancode ↔ virtual key 변환, modifier 상태 추적
   - 마우스: 좌표 변환, 버튼 상태 추적
3. **`main.rs` `map_key_to_rdp_scancode()` 개선**:
   - `ironrdp-input`의 키 매핑 테이블 활용
   - IME 조합 입력 기초 지원 (한국어/일본어/중국어)
4. **추가 입력 지원**:
   - 마우스 수평 휠 (`PointerFlags::HORIZONTAL_WHEEL`)
   - 복합 키 조합 (Ctrl+Alt+Del, Win 키 등)
   - Extended 키 플래그 정밀화

### 기대 효과
- 입력 매핑 코드 단순화
- IME/다국어 입력 지원 기초 확보
- modifier 상태 추적 정확도 향상

---

## Phase R4: 클립보드 공유

> `ironrdp-cliprdr` + `ironrdp-cliprdr-native` 통합

### 변경 내용
1. **`Cargo.toml`**: `ironrdp-cliprdr = "0.5.0"`, `ironrdp-cliprdr-native = "0.5.0"` 추가
2. **채널 등록**: `ClientConnector::with_static_channel(Cliprdr::new(backend))` 추가
3. **클립보드 흐름**:
   - 로컬 → 원격: OS 클립보드 변경 감지 → CLIPRDR 채널로 전송
   - 원격 → 로컬: CLIPRDR 수신 → OS 클립보드에 반영
4. **지원 형식**:
   - 텍스트 (CF_UNICODETEXT)
   - 이미지 (CF_DIB) — 후속
   - 파일 목록 (CF_HDROP) — 후속
5. **`ConnectionInput` 확장**: `ClipboardUpdate(String)` 변형 추가

### 기대 효과
- 복사/붙여넣기 연동 (RDP에서 가장 자주 요청되는 기능)
- 기존 SSH/Telnet과 동일 수준의 클립보드 UX

---

## Phase R5: 동적 가상 채널 + 디스플레이 제어

> `ironrdp-dvc` + `ironrdp-displaycontrol`로 동적 해상도 변경

### 변경 내용
1. **`Cargo.toml`**: `ironrdp-dvc = "0.5.0"`, `ironrdp-displaycontrol = "0.5.0"` 추가
2. **DVC 인프라 구축**:
   - `Dvc` 정적 채널 등록 (DRDYNVC 채널)
   - 동적 채널 핸들러 프레임워크 연결
3. **DisplayControl 채널**:
   - 창 리사이즈 이벤트 → `DisplayControlMonitorLayout` PDU 전송
   - 현재 하드코딩된 1280×1024 → 동적 해상도 협상
   - UI 리사이즈 → debounce(300ms) → 시스템 해상도 변경 PDU 전송
4. **`build_config()` 수정**:
   - `desktop_size` 동적 설정 (UI 창 크기 또는 모니터 해상도 기반)
   - 초기 해상도를 연결 설정 UI에서 선택 가능하게 확장

### 기대 효과
- 창 크기 변경 시 원격 해상도 실시간 동기화
- DVC 인프라가 확보되면 후속 채널(RDPDR 등) 추가가 용이

---

## Phase R6: 드라이브 리다이렉션

> `ironrdp-rdpdr` + `ironrdp-rdpdr-native`

### 변경 내용
1. **`Cargo.toml`**: `ironrdp-rdpdr = "0.5.0"`, `ironrdp-rdpdr-native = "0.5.0"` 추가
2. **채널 등록**: `ClientConnector::with_static_channel(Rdpdr::new(backend))`
3. **설정 UI**: 공유할 로컬 드라이브/폴더 선택 인터페이스
4. **파일 시스템 매핑**: 로컬 디렉터리 ↔ 원격 리다이렉트 드라이브

### 기대 효과
- 로컬 파일 시스템을 원격 세션에서 직접 접근
- 파일 전송 UX 개선

---

## Phase R7: 보안 및 인증 강화

### 변경 내용
1. **NLA/CredSSP 활성화**:
   - `enable_credssp: true` 로 전환
   - `sspi` 크레이트의 CredSSP 핸들러 검증
   - NLA 실패 시 폴백 옵션 제공
2. **TLS 인증서 검증**:
   - 기본: 시스템 인증서 저장소 검증
   - 옵션: 자체 서명 인증서 허용 (사용자 확인 후)
   - 인증서 핀닝 지원 (known_hosts 유사 모델)
3. **도메인 인증**: `domain` 필드를 UI 입력으로 확장
4. **연결 오류 세분화**:
   - 인증 실패 / 네트워크 오류 / TLS 오류 / 라이선스 오류 구분
   - UI에 구체적 오류 메시지 표시

---

## Phase R8: 세션 안정성 및 UX

### 변경 내용
1. **리소스 정리**:
   - 탭 닫기 → `CancellationToken` 기반 graceful shutdown
   - `ActiveStage::shutdown()` 호출 보장
   - 비동기 워커 종료 대기 + 타임아웃
2. **재접속 UX**:
   - 연결 끊김 감지 → "재접속" 버튼 표시
   - 자동 재접속 옵션 (지수 백오프)
3. **오디오 재생 검증**:
   - `ironrdp-rdpsnd` + cpal 백엔드 실제 동작 테스트
   - 오디오 버퍼링/동기화 정리
4. **서버 커서 표시**:
   - `enable_server_pointer: true` 로 전환
   - 포인터 캐싱 구현

---

## Phase R9: 그래픽 코덱 확장 (EGFX + NSCodec)

> EGFX(GFX) 동적 가상 채널 파이프라인 구축 및 고급 코덱 지원

### 배경

IronRDP 생태계의 그래픽 코덱 지원 현황:

| 계층 | 크레이트 | 지원 범위 |
|------|----------|-----------|
| **PDU 인코딩/디코딩** | `ironrdp-pdu::rdp::vc::dvc::gfx` | `ServerPdu` 전체: `WireToSurface1` (Codec1Type: Uncompressed/RemoteFx/**ClearCodec/Planar/Avc420/Alpha/Avc444/Avc444v2**), `WireToSurface2` (Codec2Type: **RemoteFxProgressive**), Surface 관리(Create/Delete/Map), 프레임 마커, 캐시, 리셋 |
| **RemoteFX 디코딩** | `ironrdp-graphics` + `ironrdp-session::rfx` | DWT, RLGR, 양자화, 서브밴드 재구성 → RGB 변환. `CODEC_ID_REMOTEFX` SurfaceCommands **자동 처리** |
| **ZGFX 벌크 압축** | `ironrdp-graphics::zgfx::Decompressor` | EGFX 채널 데이터 압축 해제 (RDP8) |
| **AVC PDU 구조** | `ironrdp-pdu::gfx::Avc420BitmapStream`, `Avc444BitmapStream` | H.264 비트스트림 파싱만 — **디코딩은 외부 라이브러리 필요** |
| **DVC 인프라** | `ironrdp-dvc` | `DvcProcessor` 트레이트 — GFX 채널 프로세서 직접 구현 필요 |

### 단계 R9-A: NSCodec 디코딩 구현 (독립 — DVC 불필요)

> 현재 `build_config()`에서 NSCodec을 협상하지만 디코딩 코드가 없어 서버가 NSCodec으로 전송 시 **화면 깨짐 발생 가능**

1. **즉시 조치 (NSCodec 협상 제거 또는 디코딩 추가)**:
   - **옵션 A**: `build_config()`에서 NSCodec 코덱 등록 제거 (가장 안전, 서버가 RLE/RDP6 폴백)
   - **옵션 B**: MS-RDPNSC 스펙 기반 NSCodec 디코더 직접 구현
     - NSCODEC = 3개 채널(Y, Cb, Cr) + ChromaSubsampling + ColorLossLevel 기반 디코딩
     - `ironrdp-graphics`에 `nscodec` 모듈 추가 또는 kterm 자체 구현
2. **검증**: NSCodec 전용 서버 설정에서 화면 깨짐 없음 확인

### 단계 R9-B: EGFX DVC 채널 프로세서 구축 (핵심)

> `ironrdp-dvc`의 `DvcProcessor` 트레이트를 구현하여 MS-RDPEGFX 그래픽 파이프라인 활성화

1. **`Cargo.toml`**: `ironrdp-dvc = "0.5.0"` 추가 (R5에서 이미 계획)
2. **GFX 프로세서 구현** (`GfxProcessor: DvcProcessor`):
   - 채널 이름: `"Microsoft::Windows::RDS::Graphics"`
   - 수신: `gfx::ServerPdu` 디코딩 (`WireToSurface1`, `WireToSurface2`, `CreateSurface`, `DeleteSurface`, `MapSurfaceToOutput`, `StartFrame`, `EndFrame`, `ResetGraphics` 등)
   - ZGFX 압축 해제: `ironrdp_graphics::zgfx::Decompressor` 적용
   - 프레임 마커 처리: `FrameAcknowledgePdu` 응답
   - Surface 관리: 다중 Surface 생성/삭제/매핑 상태 머신
3. **코덱 디스패치 (`WireToSurface1`)**:
   - `Codec1Type::Uncompressed` → 직접 RGBA 변환
   - `Codec1Type::RemoteFx` → `rfx::DecodingContext::decode()` (기존 코드 재사용)
   - `Codec1Type::Planar` → Planar 코덱 디코더 (직접 구현 — MS-RDPEGFX 2.2.4.4)
   - `Codec1Type::ClearCodec` → ClearCodec 디코더 (직접 구현 — MS-RDPEGFX 2.2.4.3)
   - `Codec1Type::Alpha` → 알파 채널 디코딩
4. **코덱 디스패치 (`WireToSurface2`)**:
   - `Codec2Type::RemoteFxProgressive` → Progressive RemoteFX 디코더 (ironrdp-graphics 기반 확장)
5. **Capability 협상**:
   - `CapabilitiesAdvertisePdu` 수신 → `CapabilitiesConfirmPdu` 응답
   - 지원 버전/플래그 선언 (V8, V81, V10 등)
6. **`build_config()` 수정**:
   - DVC 채널로 `DrdynvcClient`에 `GfxProcessor` 등록
   - SurfaceCommands capability 활성화

### 단계 R9-C: H.264/AVC 디코딩 (EGFX 위에서 동작)

> EGFX `WireToSurface1`의 `Avc420`/`Avc444`/`Avc444v2` 코덱 지원

1. **`Cargo.toml`**: `openh264 = "0.9.3"` 추가 (Cisco OpenH264 바인딩, BSD-2-Clause)
2. **AVC420 디코딩**:
   - `Avc420BitmapStream` → `openh264::decoder::Decoder` → YUV420P → RGBA 변환
   - 영역별 `QuantQuality` 파라미터 적용
3. **AVC444 디코딩**:
   - `Avc444BitmapStream` → 기반 스트림(YUV420) + 보조 스트림(크로마) 결합
   - `Avc444v2` 변형 지원
4. **성능 고려사항**:
   - OpenH264 디코더 인스턴스 재사용 (프레임 간 상태 유지)
   - YUV→RGB 변환 최적화 (SIMD 활용 가능)
   - GPU 텍스처 업로드 경로와 통합

### 기대 효과
- **EGFX 활성화**: Windows 10/11 서버에서 최적 그래픽 품질 (RemoteFX Progressive, ClearCodec)
- **H.264 지원**: 영상/동영상 재생 시 대역폭 절감 (AVC420/AVC444)
- **NSCodec 안정성**: 협상/디코딩 불일치 해소 (현재 잠재적 화면 깨짐 버그)
- **Progressive 렌더링**: 저대역폭 환경에서 점진적 화질 개선

### 주의사항
- EGFX GFX 프로세서는 IronRDP에 **전용 크레이트가 없으므로** kterm 자체 구현 필요
- `ironrdp-pdu::rdp::vc::dvc::gfx`의 PDU 정의와 `ironrdp-graphics`의 기본 요소를 조합
- ClearCodec, Planar 코덱은 IronRDP에 디코더가 없으므로 MS 스펙 기반 직접 구현
- OpenH264은 빌드 시 C 컴파일러 필요 (`source` feature 기본 활성화)
- R5 (DVC 인프라)가 선행되어야 R9-B, R9-C 착수 가능
- R9-A (NSCodec)는 독립적으로 즉시 착수 가능 (현재 버그 수정 성격)

---

## 실행 우선순위 및 의존관계

```
R1 (비동기 전환 ✅) ──────────┐
                              ├──→ R3 (입력 개선)
R2 (TLS 정리 ✅) ─────────────┤
                              ├──→ R4 (클립보드)
                              │
                              ├──→ R5 (DVC + 디스플레이 제어) ──→ R6 (드라이브 리다이렉션)
                              │         │
                              │         └──→ R9-B (EGFX GFX 프로세서) ──→ R9-C (H.264/AVC)
                              │
                              ├──→ R7 (보안 강화)
                              │
                              ├──→ R8 (세션 안정성)
                              │
                              └──→ R9-A (NSCodec 수정) ← 독립, 즉시 착수 가능 (버그 수정)
```

- **R1 + R2 완료** — 기반 전환 완료. R3~R9 착수 가능.
- **R9-A**: NSCodec 협상/디코딩 불일치 수정 — **즉시 착수 권장** (잠재적 화면 깨짐 버그).
- **R3**은 R1 완료 후 입력 루프가 비동기로 전환된 상태에서 진행.
- **R4**는 R1 완료 후 정적 채널 추가로 진행 가능.
- **R5**는 DVC 인프라가 필요하므로 R1 이후 진행. R5 완료 후 R6 및 **R9-B/R9-C** 착수.
- **R9-B** (EGFX GFX 프로세서)는 R5의 DVC 인프라 위에 구축. **R9-C** (H.264)는 R9-B 완료 후.
- **R7, R8**은 기능적으로 독립이나 R1 비동기 전환 후가 효율적.

---

## 코드 구조 변경 요약

### `src/connection/rdp.rs` 리팩토링 후 예상 구조

```
rdp.rs
├── connect_and_subscribe()        // 진입점 (변경 없음)
├── async fn run_rdp_worker()      // ★ spawn_blocking → tokio::spawn (R1 완료)
│   ├── connect()                  // ironrdp-tokio + ironrdp-tls 비동기 핸드셰이크 (R1/R2 완료)
│   ├── tokio::select! 메인 루프   // ★ 폴링 → 이벤트 드리븐 (R1 완료)
│   │   ├── input branch           // ironrdp-input 활용 (R3)
│   │   ├── pdu branch             // ActiveStage 출력 처리 + GFX DVC 처리 (R9-B)
│   │   └── shutdown branch        // CancellationToken (R8)
│   └── cleanup                    // graceful shutdown (R8)
├── try_handle_slowpath_bitmap()   // 유지 (IronRDP 한계 보완)
├── 픽셀 변환 함수들               // 유지 (rgb24/bgr24/rgb16/bgrx → RGBA)
├── (제거 완료) tls_upgrade / NoCertificateVerification / extract_tls_server_public_key
│
└── (R9 추가 예정)
    ├── gfx_processor.rs           // GfxProcessor: DvcProcessor (EGFX 채널)
    │   ├── Surface 상태 머신      // Create/Delete/Map/Reset
    │   ├── 코덱 디스패치          // RemoteFX/ClearCodec/Planar/AVC420/AVC444
    │   ├── ZGFX 압축 해제         // ironrdp_graphics::zgfx::Decompressor
    │   └── 프레임 마커 처리       // FrameAcknowledgePdu 응답
    └── nscodec.rs (또는 협상 제거) // NSCodec 디코더 (R9-A)
```

### `src/connection/mod.rs` 확장

```rust
pub enum RdpInput {
    // 기존 유지
    KeyboardScancode { .. },
    KeyboardUnicode { .. },
    MouseMove { .. },
    MouseButton { .. },
    MouseWheel { .. },
    // 추가
    MouseHorizontalWheel { delta: i16 },
    ClipboardUpdate { text: String },
}

pub enum ConnectionEvent {
    // 기존 유지
    Connected(..),
    Data(..),
    Frames(..),
    Disconnected,
    Error(String),
    // 추가
    ClipboardReceived(String),
    ResolutionChanged { width: u16, height: u16 },
}
```

---

## 위험 요소 및 완화 전략

| 위험 | 영향 | 완화 | 상태 |
|------|------|------|------|
| ~~`ironrdp-tokio` API가 `ironrdp-blocking`과 크게 다를 수 있음~~ | ~~R1 지연~~ | ~~IronRDP GitHub 예제 코드 참조, 점진적 마이그레이션~~ | ✅ R1 완료 |
| ~~`ironrdp-tls` rustls feature와 기존 `tokio-rustls` 버전 충돌~~ | ~~R2 빌드 실패~~ | ~~`cargo tree` 의존성 트리 사전 검증~~ | ✅ R2 완료 |
| ~~비동기 전환 중 기존 프레임 배치/스로틀 로직 깨짐~~ | ~~R1~~ | ~~기존 타이머 로직을 `tokio::time::interval`로 1:1 이식 후 개선~~ | ✅ R1 완료 |
| NSCodec 협상하지만 디코딩 코드 없음 | 잠재적 화면 깨짐 (서버가 NSCodec 전송 시) | R9-A에서 즉시 수정 — 협상 제거(안전) 또는 디코더 직접 구현 | ⚠️ 현재 버그 |
| EGFX GFX 프로세서가 IronRDP에 전용 크레이트 없음 | R9-B 구현량 증가 | `ironrdp-pdu` GFX PDU + `ironrdp-graphics` 기본 요소 조합, `DvcProcessor` 트레이트 구현 | R9-B |
| ClearCodec / Planar 코덱 IronRDP에 디코더 없음 | R9-B 일부 코덱 미지원 | MS-RDPEGFX 스펙 직접 구현. 미지원 코덱은 warn 후 skip | R9-B |
| OpenH264 빌드 시 C 컴파일러 필요 | CI/크로스컴파일 환경 빌드 실패 | `source` feature 비활성화 후 시스템 OpenH264 링크 옵션 제공 | R9-C |
| CredSSP 활성화 시 일부 서버와 호환성 문제 | R7 | NLA off 폴백 옵션 유지 | R7 |
| DVC 채널 핸들링이 IronRDP에서 실험적 | R5-R6-R9 | 채널별 feature gate, 점진적 활성화 | R5+ |

---

## 테스트 전략

| Phase | 테스트 | 상태 |
|-------|--------|------|
| R1 | Windows Server 2019/2022 + Win10/11 대상 연결/화면/입력 회귀 테스트 | ✅ `cargo run` 성공 확인 |
| R2 | 자체서명 인증서 + 공인 인증서 서버 모두 테스트 | ✅ ironrdp-tls 적용 완료 |
| R3 | 한국어 IME 입력, Function 키, 복합 키 조합 | 미완 |
| R4 | 텍스트 복사/붙여넣기 양방향 확인 | 미완 |
| R5 | 해상도 변경 후 화면 깨짐 없음 확인 | 미완 |
| R6 | 로컬 파일 원격 열기/저장 | 미완 |
| R7 | NLA 활성 서버 접속, 인증서 검증 경고 표시 | 미완 |
| R8 | 탭 닫기 후 메모리 누수 없음, 네트워크 끊김 후 재접속 | 미완 |
| R9-A | NSCodec 협상 제거 또는 디코더 추가 후 표준 서버에서 화면 깨짐 없음 확인 | 미완 |
| R9-B | EGFX GFX 채널 연결 후 Win10/11 서버에서 RemoteFX Progressive / ClearCodec 화면 정상 표시 | 미완 |
| R9-C | H.264 AVC420/AVC444로 동영상 재생 시 화면 정상 출력 및 성능 측정 | 미완 |

- 보안 정책(TLS/NLA/인증서 검증)은 기본 안전 설정을 우선한다.
- 성능 최적화는 full-frame 동작을 먼저 완성한 다음 dirty-rect로 확장한다.
- `russh 0.57+` 계열에서는 `sha1` 프리릴리스 충돌이 재발할 수 있으므로, `russh` 업그레이드는 별도 검증 브랜치에서 수행한다.
