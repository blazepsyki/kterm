# RDP 통합 구현 계획 (IronRDP)

## RDP 프로토콜 지원 현황

### ✅ 지원

#### 연결 / 인증
| 항목 | 비고 |
|------|------|
| TCP + TLS 1.2/1.3 업그레이드 | `ironrdp-tls` 크레이트 기반 (`NoCertificateVerification` 내장, 인증서 검증은 Phase 7) |
| 사용자명/비밀번호 인증 | `Credentials::UsernamePassword` |
| 서버 인증서 공개키 추출 | CredSSP 채널 바인딩용 |
| 고정 초기 데스크톱 크기 협상 | 1280×720 하드코딩 |

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
| NSCodec 코덱 협상 | `BitmapCodecs`에 NSCodec 등록 — ⚠️ **디코딩 미구현** (서버가 NSCodec으로 전송 시 화면 깨짐 가능). IronRDP에서 **NSCodec 디코딩 추가 중** — 공식 릴리스 후 즉시 통합 예정 |
| RemoteFX (SurfaceCommands 경유) | `ironrdp-session`의 `CODEC_ID_REMOTEFX` → `rfx::DecodingContext` (DWT+RLGR+양자화+서브밴드 재구성) — **기본 지원, 자동 처리** |
| Dirty Rect 단위 부분 업데이트 | Arc CoW 버퍼 + GPU 부분 텍스처 업로드 |
| 프레임 배치 병합 | ≈60fps 상한, 16ms/50ms 이중 타이머 |
| wgpu GPU 텍스처 렌더러 | WGSL 쉐이더, 뷰포트 스케일링 |
| Performance Flag 협상 | `ENABLE_FONT_SMOOTHING`, `ENABLE_DESKTOP_COMPOSITION` |

#### 입력
| 항목 | 비고 |
|------|------|
| 키보드 스캔코드 FastPath | `FastPathInputEvent::KeyboardEvent` |
| 키보드 유니코드 FastPath | `FastPathInputEvent::UnicodeKeyboardEvent` |
| XRDP NumLock 충돌 완화 | NumPad/Navigation 충돌 스캔코드(`0x47..0x53`) key-down 직전 `TS_SYNC_EVENT` 전송 |
| 원격 IME commit 입력 | Iced `InputMethod::Commit` 문자열을 RDP Unicode key down/up 시퀀스로 전송 |
| 원격 Secure Attention alias | `Ctrl+Alt+End` 입력을 원격 `Ctrl+Alt+Del`의 Delete 구간으로 매핑 |
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
| 비동기 Tokio I/O | `ironrdp-tokio` `MovableTokioStream` + `tokio::select!` 이벤트 드리븐 루프 (Phase 1 완료) |
| TLS 업그레이드 | `ironrdp-tls` 크레이트 기반 (Phase 1 완료) |

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
| **EGFX 그래픽 파이프라인 (DVC)** | `ironrdp-dvc 0.5.0` + `ironrdp-pdu` gfx 타입 + `ironrdp-graphics::zgfx` 조합으로 **Uncompressed 코덱까지 즉시 구현 가능** (Phase 9-B-1). AVC420은 ironrdp-egfx 게시 후 (Phase 9-B-2) |
| RemoteFX Progressive (EGFX 경유) | `Codec2Type::RemoteFxProgressive` — DVC GFX 프로세서 필요 |
| ClearCodec (EGFX 경유) | `Codec1Type::ClearCodec` — **IronRDP에서 추가 중**, DVC GFX 프로세서 필요 |
| Planar Codec (EGFX 경유) | `Codec1Type::Planar` — DVC GFX 프로세서 필요 |
| H.264/AVC420 (EGFX 경유) | `Codec1Type::Avc420` — **IronRDP에서 추가 중** (`openh264` 직접 통합 보류) |
| H.264/AVC444 (EGFX 경유) | `Codec1Type::Avc444` / `Avc444v2` — **IronRDP에서 추가 중** (`openh264` 직접 통합 보류) |
| NSCodec 디코딩 | 협상만 구현, **IronRDP에서 디코딩 추가 중** — 공식 릴리스 후 통합 예정 |
| ZGFX 벌크 압축 해제 (EGFX) | `ironrdp-graphics::zgfx::Decompressor` 존재 — GFX 프로세서 내 통합 필요 |
| GDI/GDI+ 그래픽 가속 Order | 미처리 |
| 서버 커서 표시 | `enable_server_pointer: false` |
| 포인터(커서) 캐싱 | |

#### 입력
| 항목 | 비고 |
|------|------|
| 마우스 수평 휠 | `PointerFlags::HORIZONTAL_WHEEL` 미구현 |
| IME 조합 입력 | commit 문자열 전송만 지원, 조합 상태/후보창/세밀한 locale 정책은 미처리 |
| 복합 키 조합 정밀 매핑 | `Ctrl+Alt+End -> Ctrl+Alt+Del`만 지원, 나머지 복합 조합 정책은 미완 |
| 서버 lock-state 전환의 프로토콜 기반 감지 | 테스트한 XRDP(LXQt)는 로그인 → 데스크톱 전환 시 `DeactivateAll`/`SetKeyboardIndicators`를 보내지 않아 불가 |
| NumLock 불일치의 완전 자동 복구 | 현재는 NumPad/Navigation 충돌 키에서만 pre-keydown sync 적용 |
| 멀티터치 | |

#### 채널 / 리다이렉션
| 항목 | 비고 |
|------|------|
| 클립보드 공유 (RDPCLIP) | **Phase 4-1 완료**: `ironrdp-cliprdr` + `ironrdp-cliprdr-native` 기반 Windows 경로 구현 및 텍스트 복사/붙여넣기 확인. **Phase 4-2 필요**: Linux/macOS 백엔드 설계/구현 |
| 드라이브 리다이렉션 (RDPDR) | 미구현 |
| 프린터 리다이렉션 | 미구현 |
| 포트/COM 리다이렉션 | 미구현 |
| USB 리다이렉션 | 미구현 |
| 동적 가상 채널 (DVC) 인프라 | DRDYNVC 정적 채널 등록 완료. **알려진 버그**: `ironrdp-dvc 0.5.0`이 DYNVC_CAPS_RSP를 항상 V1 고정 전송 → xrdp처럼 V2를 요청하는 서버에서 `"Dynamic Virtual Channel version 1 is not supported"` 에러 발생. upstream 패치 필요. |
| Microsoft::Windows::RDS::DisplayControl | xrdp가 DVC로 동적 모니터 채널을 열려 하나 kterm에 핸들러 없음 → `NO_LISTENER` 응답 → xrdp 에러 `dynamic_monitor_open_response: error` 발생. 화면 동적 리사이즈 기능 차단 원인. `ironrdp-displaycontrol` 크레이트로 구현 필요. |

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
- XRDP(LXQt) 환경의 초기 NumLock 불일치 문제에 대해, 전환 PDU가 오지 않는 서버 특성을 로그로 확인했고 NumPad/Navigation 충돌 스캔코드에 한정한 pre-keydown `TS_SYNC_EVENT` 절충안을 적용.
- **[2026-03-26 PDU 트레이스 분석]** XRDP DRDYNVC 협상 실태 확인:
  - PDU #2: xrdp → DYNVC_CAPS_REQ V2 전송. `ironrdp-dvc 0.5.0`은 V1 고정 응답 → xrdp `"Dynamic Virtual Channel version 1 is not supported"` 에러. **해결 방법**: upstream 이슈이므로 `ironrdp-dvc` 버전 업 또는 패치 필요.
  - PDU #49: xrdp → DYNVC_CREATE_REQ `Microsoft::Windows::RDS::DisplayControl` (ch_id=1); kterm NO_LISTENER 응답 → xrdp `dynamic_monitor_open_response: error` (화면 동적 리사이즈 불가 원인)
- **[2026-03-26 웹 검색 결과]** xrdp MS-RDPEGFX 지원 여부 확인:
  - **xrdp 0.10.x는 MS-RDPEGFX를 완전히 지원함** (GitHub issue #3540 "egfx unexpected re-init error when resizing", issue #3711 "H.264 codec" 등 다수 이슈·수정 이력으로 확인).
  - xrdp EGFX 초기화 순서: 연결 초반 로그인 화면 단계에서 `xrdp_egfx_create` → `DYNVC_CREATE_REQ "Microsoft::Windows::RDS::Graphics"` (DVC ch_id=1) → 클라이언트 CAPS_ADVERTISE → xrdp CAPS_CONFIRM 순으로 진행. DisplayControl 채널(ch_id=2)은 그 이후에 열림 → **EGFX와 DisplayControl 실패는 독립적**.
  - **현재 kterm에서 EGFX DVC가 열리지 않는 유일한 원인은 ironrdp-dvc V2 버그임** (DRDYNVC 협상 자체 실패). V2 버그 해결 시 xrdp는 GFX DVC를 즉시 열 것으로 예상됨.
- 원격 디스플레이 세션에서 Iced IME commit 문자열을 RDP Unicode 입력으로 전송하도록 분기 처리 완료.
- 원격 Secure Attention alias로 `Ctrl+Alt+End`를 `Ctrl+Alt+Del`의 Delete 입력으로 매핑 완료.
- 다중 픽섹 포맷 디코딩(RDP6 32bpp, RLE 16/24bpp, 비압축 BGRX/RGB565) 완료.
- wgpu GPU 텍스처 + WGSL 쉐이더 기반 `RdpPipeline` 렌더러 완료.
- Dirty Rect 단위 부분 텍스처 업로드(GPU 대역폭 최소화) 완료.
- 프레임 배치 병합(연속 Frames 이벤트 통합, ≈60fps 상한 스로틀링) 완료.
- Slow-path 비트맵 업데이트 폴백 처리(`try_handle_slowpath_bitmap`) 완료.
- RDP 오디오 재생: `ironrdp-rdpsnd` + `ironrdp-rdpsnd-native`(cpal 백엔드) 정적 채널로 연결 설정에 통합 완료. **실제 동작 테스트 미완료.**
- **[Phase 1 완료]** `ironrdp-blocking` → `ironrdp-tokio` 비동기 전환: `spawn_blocking` 제거, `tokio::spawn(async)` 전환, `tokio::select!` 기반 PDU 루프 적용.
- **[Phase 1 완료]** `ironrdp_tokio::MovableTokioStream` 기반 `UpgradedFramed` 타입 전환, `ReqwestNetworkClient` 교체.
- **[Phase 1 완료]** 수동 TLS 코드 제거(`tls_upgrade`, `extract_tls_server_public_key`, `mod danger::NoCertificateVerification` — 약 85줄) → `ironrdp-tls` 크레이트 일괄 교체.
- **[Phase 1 완료]** `Cargo.toml` 직접 의존성 정리: `ironrdp-blocking`, `tokio-rustls`, `x509-cert`, `sspi` 직접 선언 제거; `ironrdp-tokio = "0.8.0"` (reqwest feature), `ironrdp-tls = "0.2.0"` (rustls feature) 추가.
- **[R9-B-1 코드 완료 2026-03-26]** EGFX GFX DVC 프로세서 구현: `ironrdp-dvc = "0.5.0"` 추가, `GfxProcessor: DvcClientProcessor` 구현 (ZGFX + Uncompressed 코덱 + Surface 상태 머신 + FrameAcknowledge). `DrdynvcClient`에 `GfxProcessor` 등록 완료. **
- **[R8-a 코드 완료 2026-03-26]** 탭 닫기 시 `session.sender = None` 명시적 drop 추가 — worker `rx.recv()` 채널이 즉시 닫히도록 수정. **실제 리소스 정리 확인은 추후 진행.**

## 아키텍처 방향
- **단기**: 기존 `ConnectionEvent` 채널을 재사용해 연결 수명주기 안정화.
- **중기**: 그래픽 전용 이벤트 모델 도입.
  - 예시: `RdpEvent::Frame`, `RdpEvent::Pointer`, `RdpEvent::Disconnected`, `RdpEvent::Error`
- **장기**: 터미널 탭과 원격 그래픽 탭을 렌더링 레벨에서 분리.

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

## 검증 체크리스트
- [x] RDP 접속 성공 시 원격 화면이 표시된다.
- [x] 키보드/마우스 입력이 원격 세션에서 정상 동작한다.
- [ ] 탭 닫기 후 백그라운드 스레드/채널이 정상 종료된다. (코드 수정 완료 2026-03-26, **실제 종료 확인은 추후**)
- [x] 기존 SSH/Telnet/Serial/Local 동작에 회귀가 없다.
- [ ] RDP 오디오가 원격 세션 재생음을 로컬에서 출력한다. (코드 통합 완료, 테스트 미완료)
- [x] 공통 렌더링 모듈이 RDP/VNC 모두에서 재사용 가능하다.
- [ ] 프로토콜 추가 시 UI 계층 수정이 최소화된다.
- [ ] EGFX GFX DVC 채널 동작 확인 — **xrdp 0.10.x+는 MS-RDPEGFX 지원함** (GitHub issue #3540, #3711 및 xrdp 로그 분석으로 확인). 현재 블로커: `ironrdp-dvc V2 버그` 미수정 → DRDYNVC 협상 실패 → xrdp가 GFX DVC 채널을 열지 못함. **V2 버그 해결 후** xrdp는 연결 초반(로그인 전)에 `DYNVC_CREATE_REQ "Microsoft::Windows::RDS::Graphics"` 전송 → `GfxProcessor::start()` 호출 → `[GFX] channel opened` 로그 및 화면 품질 향상 확인 가능.
- [ ] DRDYNVC V2 핸드셰이크 수정 — `ironrdp-dvc 0.5.0` upstream 버그: V2 요청에 V1 고정 응답. upstream PR/버전업 필요. xrdp에서 `"Dynamic Virtual Channel version 1 is not supported"` 에러 발생 중.
- [ ] DisplayControl DVC 채널 핸들러 구현 — 현재 `NO_LISTENER` 응답으로 xrdp `dynamic_monitor_open_response: error` 발생. 화면 동적 리사이즈(`encode_resize`) 연동 시 함께 구현 필요.

## 주의사항
- 초기에 RDP는 터미널 바이트 스트림과 모델이 달라서 별도 이벤트 계층이 필요하다.
- 테스트한 XRDP(LXQt) 조합은 로그인 화면 → 데스크톱 전환 시 세션 재활성화 PDU를 보내지 않았다. 따라서 이 구간의 NumLock 상태 변화는 화면 업데이트만으로 나타나며, 프로토콜 이벤트 기반 자동 감지는 기대할 수 없다.
- 이 제약 때문에 현재 구현은 문제를 일으키는 keypad/navigation 충돌 키에만 `TS_SYNC_EVENT`를 선행 전송하는 방향으로 타협했다.

---

## 현재 의존성 vs 목표 의존성

| 현재 사용 | 버전 | 역할 |
|-----------|------|------|
| `ironrdp` (meta) | 0.14.0 | connector/session/graphics/pdu 재수출 |
| `ironrdp-tokio` | 0.8.0 | 비동기 I/O — `Framed`, `connect_begin/finalize` (**Phase 1 완료**) |
| `ironrdp-tls` | 0.2.0 | TLS 업그레이드 + 인증서 추출 (**Phase 1 완료**) |
| `ironrdp-core` | 0.1.5 | `ReadCursor`, `Decode` 등 기본 타입 |
| `ironrdp-rdpsnd` | 0.7.0 | 오디오 정적 채널 |
| `ironrdp-rdpsnd-native` | 0.5.0 | cpal 오디오 백엔드 |

> **제거됨 (Phase 1)**: `ironrdp-blocking`, `tokio-rustls`, `x509-cert`, `sspi` — Cargo.toml 직접 선언 제거 완료  
> `tokio-rustls`/`x509-cert`는 `sspi→reqwest→hyper-rustls` / `ironrdp-pdu` 경로로 간접 의존 잔류(바이너리에 포함)

| **추가 예정** | 버전 | 역할 |
|--------------|------|------|
| ~~`ironrdp-tokio`~~ | ~~0.8.0~~ | ~~**Tokio 비동기 I/O**~~ — ✅ **Phase 1 완료** |
| ~~`ironrdp-tls`~~ | ~~0.2.0~~ | ~~**TLS 보일러플레이트**~~ — ✅ **Phase 1 완료** |
| `ironrdp-cliprdr` | 0.5.0 | **클립보드 공유** (RDPECLIP 정적 채널) |
| `ironrdp-cliprdr-native` | 0.5.0 | **클립보드 네이티브 백엔드** (OS 클립보드 연동) |
| `ironrdp-dvc` | 0.5.0 | **동적 가상 채널** (DRDYNVC) — Phase 9-B-1에서 수동 GFX 프로세서 구현 시 **직접 추가 필요** (`DvcClientProcessor` 트레이트); Phase 5 DVC 인프라와 공유 |
| `ironrdp-displaycontrol` | 0.5.0 | **디스플레이 제어** (동적 해상도 변경, DVC 기반) |
| `ironrdp-input` | 0.5.0 | **입력 유틸리티** — 수동 FastPath 매핑 교체 |
| `ironrdp-rdpdr` | 0.5.0 | **드라이브 리다이렉션** (RDPDR 채널) |
| `ironrdp-rdpdr-native` | 0.5.0 | **드라이브 리다이렉션 네이티브 백엔드** |
| `ironrdp-egfx` | 0.1.0 (**crates.io 미게시 — 보류**) | EGFX 전체 파이프라인 — `GraphicsPipelineClient` DVC 프로세서 + ZGFX + AVC420 (openh264 feature) — **crates.io 재게시 후 통합** |
| `openh264` | — | ~~직접 추가~~ — `ironrdp-egfx` 내부 의존성으로 포함 (`openh264-bundled`/`openh264-libloading` feature 선택) |

| **직접 의존성 제거 완료** | 이유 | 비고 |
|--------------------------|------|------|
| ~~`ironrdp-blocking`~~ | `ironrdp-tokio`로 대체 — ✅ **Phase 1 완료** | 바이너리에서 완전 제거됨 |
| ~~`x509-cert`~~ (직접 선언) | `ironrdp-tls`가 내부 처리 — ✅ **Phase 1 완료** | `ironrdp-pdu` 간접 의존으로 바이너리 잡류 |
| ~~`tokio-rustls`~~ (직접 선언) | `ironrdp-tls`가 래핑 — ✅ **Phase 1 완료** | `sspi → reqwest → hyper-rustls` 간접 의존으로 바이너리 잡류 |
| ~~`sspi`~~ (직접 선언) | `ironrdp-tokio::reqwest::ReqwestNetworkClient`로 교체 — ✅ **Phase 1 완료** | 간접 의존으로 바이너리 잡류 |

> **⚠️ 의존성 분석 결과**: `cargo tree -i` 확인 결과, `tokio-rustls`는 `sspi → reqwest → hyper-rustls` 체인으로, `x509-cert`는 `ironrdp-pdu`를 통해 이미 간접 의존되고 있음. 따라서 `Cargo.toml`에서 직접 선언만 제거할 수 있으며, 두 크레이트는 컴파일된 바이너리에 계속 포함됨. **실질적 효과는 `kterm` 직접 코드에서 해당 크레이트 API 사용 제거 (코드 단순화)에 있음.**

---

## 단계별 구현 계획

---

## Phase 1: 연결 기반 구축 ✅ 완료

> 기반 연결 흐름 구축 + `ironrdp-blocking` → `ironrdp-tokio` 비동기 전환 + 수동 TLS 코드 → `ironrdp-tls` 크레이트 교체

### 완료 항목
- [x] UI에서 RDP 접속 정보 입력/전송
- [x] `ConnectRdp` 메시지 핸들러 추가
- [x] `connection::rdp::connect_and_subscribe` 연결
- [x] Cargo 의존성 해소(`russh 0.55.0` + `ironrdp 0.14.0`) 및 `cargo check` 통과
- [x] IronRDP 실제 핸드셰이크 적용(Connector + TLS upgrade + finalize)
- [x] ActiveStage 기반 그래픽 프레임 수신 루프 1차 연결(프로브/응답 프레임 송신)
- [x] ActiveStage 출력을 Iced 렌더링 상태로 직접 브리지
- [x] `ironrdp-blocking` → `ironrdp-tokio` 비동기 전환 (`tokio::spawn` + `tokio::select!`)
  - `spawn_blocking` + 동기 루프 → `tokio::spawn` + 비동기 루프
  - `ironrdp_blocking::Framed<TlsStream>` → `ironrdp_tokio::Framed<MovableTokioStream<TlsStream<TcpStream>>>`
  - `LocalTokioStream` (`!Send`) → `MovableTokioStream` (Send 바운드 충족)
  - `loop { try_recv(); read_pdu(); sleep(1ms) }` → `tokio::select! { input = rx.recv() => ..., pdu = framed.read_pdu().await => ... }`
  - `sspi::ReqwestNetworkClient` → `ironrdp_tokio::reqwest::ReqwestNetworkClient`
- [x] `ironrdp-tls` 크레이트 기반 TLS 계층 교체 (수동 보일러플레이트 ~85줄 제거)
  - 제거: `fn tls_upgrade()` (~30줄), `fn extract_tls_server_public_key()` (~15줄), `mod danger::NoCertificateVerification` (~40줄)
  - `ironrdp_tls::upgrade(stream, server_name).await` → `(TlsStream, Certificate)` 반환
  - `ironrdp_tls::extract_tls_server_public_key(&cert)` 사용
- [x] `Cargo.toml` 직접 의존성 정리
  - 제거: `ironrdp-blocking`, `tokio-rustls`, `x509-cert`, `sspi` (직접 선언)
  - 추가: `ironrdp-tokio = "0.8.0"` (reqwest feature), `ironrdp-tls = "0.2.0"` (rustls feature)
- [ ] 연결 실패/인증 실패/종료 사유 세분화

### 기대 효과
- CPU 폴링 오버헤드 제거 (idle 시 0% CPU), `spawn_blocking` 스레드풀 점유 해소
- 입력 응답 지연 최소화 (sleep(1ms) 제거)
- 향후 async 채널(DVC, CLIPRDR 등)과 자연스러운 통합

> ⚠️ **보안 참고**: `ironrdp-tls` 내부적으로 `NoCertificateVerification` 사용 중. 실질적 서버 인증서 검증 활성화는 **Phase 7**에서 진행.

---

## Phase 2: 그래픽 파이프라인 ✅ 완료

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

---

## Phase 2.5: VNC 대비 공통화 검증

- [ ] 더미 백엔드(테스트 프레임 생성기)로 공통 렌더러 단독 검증
- [ ] RDP 백엔드를 공통 렌더러에 연결
- [ ] VNC 백엔드 연결 시 코드 변경 최소화(목표: UI 코드 변경 0 또는 극소)

---

## Phase 3: 입력/상호작용 (부분 완료)

> 수동 FastPath 매핑 + `ironrdp-input` 크레이트 활용으로 단계적 개선

### 완료 항목
- [x] 키보드 입력을 RDP FastPath(스캔코드/유니코드)로 기본 매핑
- [x] 마우스 이동/클릭/휠 이벤트 기본 매핑
- [x] XRDP NumLock 불일치 완화: NumPad/Navigation 충돌 키(`0x47..0x53`) key-down 직전 `TS_SYNC_EVENT` 전송
- [x] 포커스 및 입력 캡처 정책 정리 — 원격 세션 중 모든 키보드 입력을 원격으로 전달
- [x] `ironrdp-input = "0.5.0"` 직접 의존성 추가; `rdp.rs` 입력 파이프라인에서 `Database`, `Operation`, `Scancode` 등 `ironrdp-input` 타입 직접 사용으로 전환
- [x] pre-keydown sync / modifier release 경로의 중복 로직 정리

### 미완 항목
- [ ] `main.rs` `map_key_to_rdp_scancode()` 개선 (`ironrdp-input` 키 매핑 테이블 활용)
- [ ] IME 조합 입력 기초 지원 (한국어/일본어/중국어)
- [ ] 마우스 수평 휠 (`PointerFlags::HORIZONTAL_WHEEL`)
- [ ] 복합 키 조합 정밀 매핑 (Ctrl+Alt+Del, Win 키 등)
- [ ] Extended 키 플래그 정밀화

### 기대 효과
- 입력 매핑 코드 단순화
- IME/다국어 입력 지원 기초 확보
- modifier 상태 추적 정확도 향상

**평가**: 기본 입력 경로와 XRDP NumLock 완화는 완료되었습니다. 현재 상태는 "기본 입력 동작 완료 + 특정 XRDP 결함 완화 완료"이며, `ironrdp-input` 전면 전환·IME 조합·복합 키 정책 정리는 미완입니다. XRDP NumLock 문제는 서버가 전환 PDU를 보내지 않는다는 제약이 확인되었으므로, `ironrdp-input` 도입만으로 완전히 해결되지는 않습니다.

---

## Phase 4: 클립보드 공유

> `ironrdp-cliprdr` 기반 공통 CLIPRDR 계층 + 플랫폼별 OS 클립보드 백엔드

### Phase 4-1: Windows 네이티브 백엔드

> `ironrdp-cliprdr` + `ironrdp-cliprdr-native` 통합

**상태**: 구현 및 텍스트 복사/붙여넣기 확인 완료 2026-03-26

#### 변경 내용
1. **`Cargo.toml`**: `ironrdp-cliprdr = "0.5.0"`, `ironrdp-cliprdr-native = "0.5.0"` 추가
2. **채널 등록**: `ClientConnector::with_static_channel(CliprdrClient::new(backend))` 추가
3. **클립보드 흐름**:
   - 로컬 → 원격: OS 클립보드 변경 감지 → CLIPRDR 채널로 전송
   - 원격 → 로컬: CLIPRDR 수신 → `ironrdp-cliprdr-native` Windows 백엔드가 OS 클립보드에 반영
4. **지원 형식**:
   - 텍스트 (CF_UNICODETEXT) 중심의 네이티브 백엔드 경로
   - 이미지 (CF_DIB) — 후속
   - 파일 목록 (CF_HDROP) — 후속
5. **구현 방식**:
   - `WinClipboard::new(proxy)`로 숨김 윈도우 + 클립보드 리스너 생성
   - `ClipboardMessageProxy`로 OS 이벤트를 RDP 워커 async 채널로 전달
   - 워커에서 `CliprdrClient::initiate_copy`, `submit_format_data`, `initiate_paste` 호출
6. **플랫폼 범위**:
   - 현재 앱 코드에서 `WinClipboard` 경로는 `cfg(windows)`로만 활성화
   - 비-Windows 빌드에서는 CLIPRDR 백엔드 팩토리를 연결하지 않으므로 이 경로는 동작하지 않음

#### 기대 효과
- 복사/붙여넣기 연동 (RDP에서 가장 자주 요청되는 기능)
- 기존 SSH/Telnet과 동일 수준의 클립보드 UX
- 네이티브 백엔드를 직접 재구현하지 않고 IronRDP가 제공하는 Windows 구현 재사용

#### 검증 메모
- 텍스트 복사/붙여넣기 양방향 동작 확인 완료
- 이미지/파일 목록 경로는 아직 후속 검증 필요
- 위 검증은 Windows 환경 기준

### Phase 4-2: Linux/macOS 백엔드 설계

> `ironrdp-cliprdr::backend::CliprdrBackend`를 직접 구현하는 교차 플랫폼 경로

**상태**: 설계 단계

#### 목표 범위
1. Linux(X11/Wayland) 및 macOS에서 텍스트 클립보드(CF_UNICODETEXT) 우선 지원
2. 기존 Phase 4-1의 RDP 워커 연동 방식은 유지하고, OS 클립보드 백엔드만 교체 가능하게 분리
3. 이미지/파일 목록은 4-2 1차 범위에서 제외하고 텍스트 경로 안정화 후 확장

#### 권장 구조
1. **공통 추상화 추가**:
   - `CliprdrBackendFactory`를 플랫폼별로 생성하는 앱 레벨 팩토리 계층 도입
   - `main.rs`의 `cfg(windows)` 분기를 `platform/clipboard_*` 모듈로 이동
2. **플랫폼별 백엔드**:
   - Windows: 기존 `WinClipboard` 유지
   - Linux/macOS: `CliprdrBackend` 구현체를 앱 내부에 추가
3. **공통 메시지 흐름 유지**:
   - OS 이벤트/폴링 → `ClipboardMessageProxy`
   - RDP 워커 → `CliprdrClient::{initiate_copy, submit_format_data, initiate_paste}`
   - 이 경로는 Windows 구현과 동일하게 재사용

#### Linux/macOS 구현 전략
1. **초기 범위는 텍스트 전용**:
   - 로컬 복사 시 `ClipboardFormatId::CF_UNICODETEXT`만 광고
   - 원격 paste 요청 시 UTF-16 텍스트 인코딩/디코딩만 처리
2. **OS 클립보드 접근 방식**:
   - 1순위 후보: `arboard` 기반 read/write 경로 재사용
   - 변경 감지는 플랫폼 제약 때문에 이벤트 기반보다 주기적 폴링이 현실적
   - Linux Wayland 환경은 소유권/세션 제약이 있어 폴링 실패 또는 권한 이슈를 별도 처리해야 함
3. **백엔드 이벤트 루프**:
   - Tokio task 또는 전용 스레드에서 마지막 텍스트 값 해시/스냅샷 비교
   - 변경 시 `ClipboardMessage::SendInitiateCopy(vec![CF_UNICODETEXT])` 전송
   - `on_format_data_request`에서 현재 로컬 텍스트를 `OwnedFormatDataResponse`로 변환
   - `on_format_data_response`에서 원격 텍스트를 로컬 OS 클립보드에 반영

#### 설계 시 주의점
1. **Wayland 제약**:
   - 백그라운드 앱의 clipboard ownership과 pasteboard 접근이 compositor 정책에 의해 제한될 수 있음
   - 필요 시 X11/Wayland 지원 상태를 분리 표기해야 함
2. **macOS 제약**:
   - 앱 포커스/런루프와 pasteboard 갱신 타이밍 차이 검증 필요
3. **중복 전송 루프 방지**:
   - 원격에서 받은 텍스트를 로컬에 쓴 직후 다시 로컬 변경으로 감지해 역전송하지 않도록 suppression token 필요
4. **현재 UI clipboard와 역할 분리**:
   - Iced clipboard는 사용자 명시 복사/붙여넣기용으로 남기고, Phase 4-2는 세션 동기화 백엔드로 별도 유지하는 편이 안전

#### 예상 구현 단계
1. `platform/clipboard/mod.rs` 도입: 공통 팩토리 인터페이스 정의
2. `platform/clipboard_windows.rs`: 현재 `WinClipboard` 연동 코드 이동
3. `platform/clipboard_unix.rs`: Linux/macOS용 `CliprdrBackend` + 폴링 루프 구현
4. `main.rs`: 플랫폼별 백엔드 생성 호출만 남기도록 단순화
5. Linux(X11/Wayland), macOS 각각에서 텍스트 양방향 검증

#### 완료 조건
- Linux에서 텍스트 복사/붙여넣기 양방향 확인
- macOS에서 텍스트 복사/붙여넣기 양방향 확인
- Windows Phase 4-1 경로와 동일한 RDP 워커 인터페이스 유지

---

## Phase 5: 동적 가상 채널 + 디스플레이 제어

> `ironrdp-dvc` + `ironrdp-displaycontrol`로 동적 해상도 변경

### 변경 내용
1. **`Cargo.toml`**: `ironrdp-dvc = "0.5.0"`, `ironrdp-displaycontrol = "0.5.0"` 추가
2. **DVC 인프라 구축**:
   - `Dvc` 정적 채널 등록 (DRDYNVC 채널)
   - 동적 채널 핸들러 프레임워크 연결
3. **DisplayControl 채널**:
   - 창 리사이즈 이벤트 → `DisplayControlMonitorLayout` PDU 전송
   - 현재 하드코딩된 1280×720 → 동적 해상도 협상
   - UI 리사이즈 → debounce(300ms) → 시스템 해상도 변경 PDU 전송
4. **`build_config()` 수정**:
   - `desktop_size` 동적 설정 (UI 창 크기 또는 모니터 해상도 기반)
   - 초기 해상도를 연결 설정 UI에서 선택 가능하게 확장

### 기대 효과
- 창 크기 변경 시 원격 해상도 실시간 동기화
- DVC 인프라가 확보되면 후속 채널(RDPDR 등) 추가가 용이

---

## Phase 6: 드라이브 리다이렉션

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

## Phase 7: 보안 및 인증 강화

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

## Phase 8: 세션 안정성 및 UX

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

## Phase 9: 그래픽 코덱 확장 (EGFX + NSCodec)

> EGFX(GFX) 동적 가상 채널 파이프라인 구축 및 고급 코덱 지원

### 배경

IronRDP 생태계의 그래픽 코덱 지원 현황 (2026-03-26 기준):

| 구분 | 코덱 명칭 | IronRDP 지원 상태 | 비고 |
|------|-----------|-------------------|------|
| **기본 지원** | Uncompressed Raw Bitmap | ✅ 지원 | 압축되지 않은 원시 비트맵 데이터 전달 |
| | Interleaved RLE | ✅ 지원 | 런 길이 인코딩(Run-Length Encoding) 방식의 비트맵 코덱 |
| | RDP 6.0 Bitmap Compression | ✅ 지원 | 표준 RDP 비트맵 압축 방식 |
| | Microsoft RemoteFX (RFX) | ✅ 지원 | 고성능 그래픽 전송을 위한 웨이블릿 기반 코덱 |
| **추가 중** | NSCodec ([MS-RDPNSC]) | 🔧 추가 중 | 화면 이미지를 효율적으로 압축하기 위한 확장 코덱 |
| | ClearCodec | 🔧 추가 중 | 텍스트 및 UI 요소에 최적화된 무손실 코덱 |
| | AVC420 / AVC444 | 🔧 추가 중 | H.264 기반의 비디오 스트리밍 코덱 |

> **핵심 변경**: NSCodec, ClearCodec, AVC420/AVC444가 IronRDP에서 **공식적으로 추가 작업 중**이므로, kterm에서 이들 코덱을 직접 구현할 필요가 없어짐. IronRDP 공식 릴리스 후 크레이트 업데이트만으로 통합 가능.  
> **Phase 9 구성**: Phase 9-B-1 (published crates 기반 Uncompressed 코덱 수동 GFX 프로세서 — **즉시 구현 가능**) → Phase 9-B-2 (ironrdp-egfx 기반 교체 + AVC420 — ⏸ 보류) → Phase 9-C (ClearCodec + AVC444 통합 대기) → Phase 9-A (NSCodec 통합 대기)

#### IronRDP 크레이트별 지원 범위 (기존)

| 계층 | 크레이트 | 지원 범위 |
|------|----------|-----------|
| **PDU 인코딩/디코딩** | `ironrdp-pdu::rdp::vc::dvc::gfx` | `ServerPdu` 전체: `WireToSurface1` (Codec1Type: Uncompressed/RemoteFx/**ClearCodec/Planar/Avc420/Alpha/Avc444/Avc444v2**), `WireToSurface2` (Codec2Type: **RemoteFxProgressive**), Surface 관리(Create/Delete/Map), 프레임 마커, 캐시, 리셋 |
| **RemoteFX 디코딩** | `ironrdp-graphics` + `ironrdp-session::rfx` | DWT, RLGR, 양자화, 서브밴드 재구성 → RGB 변환. `CODEC_ID_REMOTEFX` SurfaceCommands **자동 처리** |
| **ZGFX 벌크 압축** | `ironrdp-graphics::zgfx::Decompressor` | EGFX 채널 데이터 압축 해제 (RDP8) |
| **AVC PDU 구조** | `ironrdp-pdu::gfx::Avc420BitmapStream`, `Avc444BitmapStream` | H.264 비트스트림 파싱 |
| **EGFX 파이프라인 (신규 크레이트)** | `ironrdp-egfx` (GitHub master, **crates.io 미게시**) | `GraphicsPipelineClient` (`DvcClientProcessor` 완전 구현체) + MS-RDPEGFX 전체 PDU 23종 + ZGFX 내장 + AVC420 feature 지원 (`openh264-bundled`/`openh264-libloading`) — 약 1달 전 추가됨 (PR #1057) |
| **NSCodec** | (추가 중) | IronRDP에서 디코딩 구현 추가 작업 진행 중 |
| **ClearCodec** | (추가 중) | IronRDP에서 디코딩 구현 추가 작업 진행 중 |
| **DVC 인프라 + 수동 GFX 프로세서** | `ironrdp-dvc` | `DvcProcessor` / `DvcClientProcessor` 트레이트 — Phase 9-B-1 수동 구현 시 직접 사용; Phase 9-B-2에서 ironrdp-egfx 내부 의존성으로도 포함 |

### 단계 Phase 9-A: NSCodec 디코딩 통합 (IronRDP 추가 중 — DVC 불필요)

> IronRDP에서 NSCodec 디코딩을 **공식 추가 작업 중**. kterm 자체 구현 불필요.
> 현재 `build_config()`에서 NSCodec 협상은 비활성화 상태 — IronRDP 공식 릴리스 후 활성화 예정.

1. **현재 방향 (변경됨)**:
   - IronRDP에서 NSCodec 디코딩이 **추가 작업 중**이므로, kterm 자체 fallback 디코더 구현 계획은 완전히 철회.
   - IronRDP의 NSCodec 지원이 포함된 버전이 릴리스되면 `Cargo.toml` 크레이트 버전 업데이트 + 협상 활성화만으로 통합 가능.
2. **통합 시 작업**:
   - `Cargo.toml`에서 `ironrdp` / `ironrdp-graphics` 버전을 NSCodec 지원 버전으로 업데이트
   - `build_config()`에서 NSCodec 협상 재활성화 (`BitmapCodecs`에 NSCodec 등록)
   - 디코딩은 IronRDP 내부에서 자동 처리 — kterm 추가 코드 불필요 예상
3. **남은 검증**:
   - 현재 설정(비-NSCodec 협상)에서 표준 서버 회귀 확인
   - IronRDP NSCodec 지원 릴리스 추적 및 적용 시점 확정
   - NSCodec 활성화 후 실제 서버에서 화면 품질/안정성 검증

### 단계 Phase 9-B-1: 최소 수동 GFX 프로세서 구현 (published crates 기반) — ✅ **즉시 구현 가능**

> `ironrdp-egfx` 없이도 **현재 crates.io에 게시된 크레이트만으로** Uncompressed 코덱 지원 EGFX 프로세서를 구현할 수 있다.
> `ironrdp-dvc 0.5.0` + `ironrdp-pdu::rdp::vc::dvc::gfx` (PDU 전체 타입) + `ironrdp-graphics::zgfx::Decompressor` 직접 조합.

#### 즉시 구현 가능한 범위 (published crates 기반)

| 기능 | 사용 크레이트 | 비고 |
|------|--------------|------|
| GFX DVC 채널 등록 | `ironrdp-dvc 0.5.0` — `DvcClientProcessor` 트레이트 직접 구현 | 채널명: `"Microsoft::Windows::RDS::Graphics"` |
| ZGFX 벌크 압축 해제 | `ironrdp-graphics::zgfx::Decompressor` | `ironrdp-egfx` 내부와 동일한 구현체 |
| GFX PDU 파싱 | `ironrdp-pdu::rdp::vc::dvc::gfx` — 23종 PDU 타입 | `WireToSurface1Pdu`, `FrameMarkerPdu`, `ServerPdu` 등 |
| Uncompressed 코덱 디코딩 | `Codec1Type::Uncompressed` — PDU payload에서 픽셀 데이터 직접 추출 | 별도 코덱 라이브러리 불필요 |
| Surface 상태 관리 | `BTreeMap<u16, Surface>` 직접 구현 | `CreateSurface` / `DeleteSurface` / `MapSurfaceToOutput` 처리 |
| Capability 협상 | `CapabilitiesAdvertisePdu` + `CapabilitySet::V8` | IronRDP PDU 타입으로 직접 인코딩 |
| FrameAcknowledge 전송 | `ClientPdu::FrameAcknowledge(FrameAcknowledgePdu { ... })` | `FrameMarker::End` 수신 시 응답 |

#### ironrdp-egfx 없이는 불가능한 범위 (Phase 9-B-2 / Phase 9-C 대기)

| 기능 | 이유 |
|------|------|
| AVC420 / H.264 디코딩 | `openh264` 통합 코드가 `ironrdp-egfx` 내부에만 존재 |
| ClearCodec 디코딩 | IronRDP 추가 작업 중 (미게시) |
| AVC444 디코딩 | IronRDP 추가 작업 중 (미게시) |
| Surface 캐시 합성 (`CacheToSurface`) | 복잡한 캐시 상태 머신 — `ironrdp-egfx`가 완전 처리 |

#### 구현 구조

```rust
struct GfxProcessor {
    decompressor: ironrdp_graphics::zgfx::Decompressor,
    surfaces: BTreeMap<u16, Surface>,
    // 렌더링 파이프라인 채널 송신자
    frame_tx: tokio::sync::mpsc::Sender<FrameUpdate>,
}

impl DvcProcessor for GfxProcessor {
    const CHANNEL_NAME: &'static str = "Microsoft::Windows::RDS::Graphics";
    // ...
}

impl DvcClientProcessor for GfxProcessor {
    fn process(&mut self, _channel_id: DynamicChannelId, payload: &[u8]) -> PduResult<Vec<DvcMessage>> {
        let data = self.decompressor.decompress(payload)?;
        let pdu = ironrdp_pdu::decode::<ServerPdu>(&data)?;
        match pdu {
            ServerPdu::WireToSurface1(p) if p.codec_id == Codec1Type::Uncompressed => {
                // 픽셀 데이터 직접 추출 → FrameUpdate::Rect 생성
            }
            ServerPdu::FrameMarker(p) if p.frame_action == FrameAction::End => {
                // FrameAcknowledgePdu 응답 반환
            }
            ServerPdu::WireToSurface1(p) => {
                // 미지원 코덱 경고 로그 (AVC420, ClearCodec 등)
            }
            // CreateSurface / DeleteSurface / MapSurfaceToOutput / ResetGraphics ...
        }
        Ok(responses)
    }
}
```

#### 작업 목록 ✅ 2026-03-26 구현 완료 (검증 추후)

1. ~~`Cargo.toml`에 `ironrdp-dvc = "0.5.0"` 직접 추가 (Phase 5와 공유)~~ ✅ 완료
2. ~~`src/connection/rdp.rs`에 `GfxProcessor: DvcClientProcessor` 구현~~ ✅ 완료
   - ZGFX 전처리 (`Decompressor` 내장)
   - `ServerPdu` 디코딩 분기문
   - `Codec1Type::Uncompressed` — `FrameUpdate::Rect` 생성
   - Surface 상태 머신 (Create / Delete / Map / ResetGraphics)
   - `FrameAcknowledge` 자동 전송
   - Capability 협상 (V8 기본)
3. ~~`build_config()`에서 EGFX DVC 채널 등록 (`DrdynvcClient`에 `GfxProcessor` 추가)~~ ✅ 완료
4. 미지원 코덱 수신 시 경고 로그 (`eprintln!`) ✅ 완료 (Phase 9-B-2 / Phase 9-C 에서 제거 예정)

> **실제 서버 동작 확인**: Win10/11 XRDP 서버 접속 후 `[GFX] channel opened` 로그 + 화면 품질 변화 검증 — **추후 진행**

> **ironrdp-egfx 게시 후**: Phase 9-B-2에서 `GfxProcessor`를 `GraphicsPipelineClient`로 교체 → AVC420 추가, Surface 쾐시 처리 개선

---

### 단계 Phase 9-B-2: ironrdp-egfx 기반 전면 교체 + AVC420 추가 — ⏸ **보류** (`ironrdp-egfx` crates.io 게시 대기)

> `ironrdp-egfx` 크레이트가 현재 **재작업 중** (`publish = false`). crates.io 게시 완료 후 착수.
> Phase 9-B-1에서 구현한 수동 `GfxProcessor`를 `GraphicsPipelineClient`로 교체하고, AVC420을 추가하는 단계.
> 이 단계는 **ironrdp-egfx가 이미 지원하는 코덱**만 대상으로 함 — 추가 중인 코덱(ClearCodec, AVC444)은 Phase 9-C 참조.

#### `ironrdp-egfx` 크레이트 현황 (2026-03-26)
- **저장소**: `crates/ironrdp-egfx` in IronRDP GitHub (PR #1057, ~1달 전 추가)
- **crates.io 게시**: ❌ 미게시 — 재작업 완료 후 재게시 예정
- **제공 API**: `GraphicsPipelineClient` (`DvcClientProcessor` 완전 구현체), `GraphicsPipelineHandler`, `BitmapUpdate`, `Surface`, `CodecCapabilities`
- **내장 기능**: ZGFX 압축 해제, 능력 협상(V8~V10.7), Surface 상태 관리, FrameAcknowledge 자동 전송

#### 이 단계에서 추가할 코덱 범위 (Phase 9-B-1 대비)
| 코덱 | ironrdp-egfx 상태 | Phase 9-B-1과의 차이 |
|------|-------------------|-----------------|
| `Codec1Type::Uncompressed` | ✅ 지원 | Phase 9-B-1에서 수동 구현 → `GraphicsPipelineClient` 자동 처리로 교체 |
| `Codec1Type::Avc420` | ✅ 지원 (openh264 feature) | Phase 9-B-1에서 경고 로그 → `openh264-libloading` feature + `OpenH264Decoder` 주입 시 자동 디코딩 |

#### 게시 후 통합전략 (사전 정리)
```
Phase 9-B-1:     kterm의 수동 GfxProcessor (Uncompressed만 지원, ~수백 줄)
Phase 9-B-2:     ironrdp-egfx::GraphicsPipelineClient로 교체 + GraphicsPipelineHandler 구현 (kterm 고유 로직만)
효과:       Surface 캐시, FrameAcknowledge, ZGFX 등을 GraphicsPipelineClient가 자동 처리 → kterm 코드 대폭 단순화
```

게시 후 작업:
1. `Cargo.toml`에 `ironrdp-egfx = { version = "0.1.0", features = ["openh264-libloading"] }` 추가
2. `EgfxHandler: GraphicsPipelineHandler` 구현
   - `on_bitmap_updated(&BitmapUpdate)` → `FrameUpdate::Rect` 생성 → 렌더링 파이프라인 전달
   - `on_reset_graphics(width, height)` → 프레임 버퍼 리셋 + `FrameUpdate::Resize` 전달
   - `on_frame_complete` → 배치 병합 타이머 연동
   - `on_unhandled_pdu` → ClearCodec/AVC444/Planar 등 경고 로그 출력
3. DVC 채널에 `GraphicsPipelineClient` 등록 (`DrdynvcClient`)
4. `build_config()` 수정 — EGFX 활성화

### 단계 Phase 9-C: EGFX 추가 코덱 (ClearCodec + AVC444) — ⏸ **보류** (IronRDP/`ironrdp-egfx` 추가 중 대기)

> Phase 9-B 완료 후, **IronRDP에서 추가 작업 중**인 코덱이 `ironrdp-egfx`에 포함되면 kterm `GraphicsPipelineHandler`의 `on_unhandled_pdu` 경로에서 자동 처리 경로로 전환됨.

#### 대기 중인 코덱 현황
| 코덱 | IronRDP 상태 | ironrdp-egfx 상태 | kterm 대응 |
|------|-------------|-------------------|-----------|
| ClearCodec | 🔧 추가 중 | 미구현 (on_unhandled_pdu) | Phase 9-B에서 경고 로그 출력 → ironrdp-egfx 포함 시 자동 처리 |
| AVC444 / AVC444v2 | 🔧 추가 중 | 미구현 (on_unhandled_pdu) | Phase 9-B에서 경고 로그 출력 → ironrdp-egfx 포함 시 자동 처리 |
| Planar | 미지원 | 미구현 | 지원 계획 없음 (희귀 코덱) |
| RemoteFxProgressive (WireToSurface2) | 미지원 | on_wire_to_surface2 위임 | 미정 |

#### 통합 시 작업 (IronRDP/ironrdp-egfx 업데이트 후)
1. `ironrdp-egfx` 버전 업데이트 (ClearCodec/AVC444 포함 버전)
2. kterm `EgfxHandler::on_unhandled_pdu`에서 해당 코덱 경고 로그 제거
3. AVC444 지원 시 `openh264-libloading` feature가 그대로 활용됨
4. `cargo deny` 라이선스 정책 재검토 (AVC444 추가 특허/라이선스 없음 — openh264로 처리)

### 기대 효과
- **EGFX 활성화**: Windows 10/11 서버에서 최적 그래픽 품질 (RemoteFX Progressive, ClearCodec)
- **H.264/AVC420 지원**: 영상/동영상 재생 시 대역폭 절감 — `ironrdp-egfx` + openh264 feature로 즉시 활성화
- **NSCodec 안정성**: 협상/디코딩 불일치 해소 (IronRDP 추가 중)
- **Progressive 렌더링**: 저대역폭 환경에서 점진적 화질 개선
- **구현 부담 대폭 감소**: kterm에서 DVC 프로세서, Surface 상태 머신, ZGFX 압축 해제, FrameAcknowledge 처리 등 직접 구현 불필요 — `GraphicsPipelineClient` + `GraphicsPipelineHandler` 패턴으로 완전 대체

### 주의사항
- ~~EGFX GFX 프로세서는 IronRDP에 전용 크레이트가 없으므로 kterm 자체 구현 필요~~ → **`ironrdp-egfx` 크레이트가 `GraphicsPipelineClient`로 완전 구현 제공** — kterm은 `GraphicsPipelineHandler` 구현만 필요
- **`ironrdp-egfx` crates.io 미게시**: Phase 9-B/Phase 9-C 모두 게시 후 착수 — git 의존성은 사용하지 않음
- **Phase 9-B 범위**: 현재 ironrdp-egfx가 지원하는 Uncompressed + AVC420만 구현 대상. ClearCodec/AVC444는 Phase 9-C로 분리
- **Phase 9-C 범위**: ironrdp-egfx에 ClearCodec/AVC444가 추가되면 `on_unhandled_pdu` 분기 제거만으로 활성화 가능
- Planar 코덱은 현재 IronRDP/ironrdp-egfx 모두 지원 계획 없음 — `on_unhandled_pdu` 위임 유지
- `openh264-bundled` feature는 Cisco 특허 커버리지 없음; 배포 환경은 `openh264-libloading` 사용
- Phase 5 (DVC 인프라 등록)가 선행되어야 Phase 9-B에서 `GraphicsPipelineClient`를 DVC에 등록 가능
- Phase 9-A (NSCodec), Phase 9-C (ClearCodec/AVC444) 모두 **IronRDP 업데이트 타이밍에 의존** — 릴리스 추적 필요

---

## 실행 우선순위 및 의존관계

```
Phase 1 (연결 기반 구축 ✅) ──────────────┬
                              ├──→ Phase 3 (입력 개선)
                              ├──→ Phase 4-1 (Windows 클립보드)
                              │         └──→ Phase 4-2 (Linux/macOS 클립보드)
                              │
                              ├──→ Phase 5 (DVC + 디스플레이 제어) ──→ Phase 6 (드라이브 리다이렉션)
                              │         │
                              │         └──→ Phase 9-B-2/Phase 9-C (⏸ ironrdp-egfx crates.io 재게시 대기)
                              │
                              ├──→ Phase 7 (보안 강화)
                              │
                              ├──→ Phase 8 (세션 안정성)
                              │
                              ├──→ Phase 9-B-1 (최소 수동 GFX, 즉시 착수 가능 ✅)
                              │         ※ published crates 기반, Phase 5 완료 불필요
                              │
                              └──→ Phase 9-A (NSCodec) ← IronRDP 릴리스 대기 (추가 중)
```

- **Phase 1 완료** — 기반 전환 완료. Phase 3~9 착수 가능.
- **Phase 9-B-1**: published crates만으로 **즉시 착수 가능** — `ironrdp-dvc 0.5.0` + `ironrdp-pdu` gfx + `ironrdp-graphics::zgfx`로 Uncompressed 코덱 EGFX 처리기 구현. Phase 5(DVC 인프라) 완료 여부와 무관하게 독립 구현 가능.
- **Phase 9-B-2 + Phase 9-C**: `ironrdp-egfx` 재작업 중 crates.io 미게시 — **게시 완료 후 Phase 5 완료 시점에 맞춰 통합**.
  - Phase 9-B-2: Phase 9-B-1 수동 구현을 GraphicsPipㅌelineClient로 교체 + AVC420 추가
  - Phase 9-C: ClearCodec + AVC444 추가 시 자동 활성화 (on_unhandled_pdu 분기 제거)
- **Phase 9-A (NSCodec)**: IronRDP에서 **추가 작업 중** — 공식 릴리스 후 크레이트 업데이트 + 협상 활성화만으로 통합.
- **Phase 3**은 Phase 1 완료 후 입력 루프가 비동기로 전환된 상태에서 진행.
- **Phase 4-1**은 완료. Windows에서 CLIPRDR 텍스트 경로 검증까지 끝남.
- **Phase 4-2**는 Phase 1 완료 후 동일한 정적 채널 구조를 재사용해 Linux/macOS 백엔드만 추가 구현하면 됨.
- **Phase 5**는 DVC 인프라가 필요하므로 Phase 1 이후 진행. Phase 5 완료 후 Phase 6 및 **Phase 9-B/Phase 9-C** 착수 (게시 시점에 연동).
- **Phase 7, Phase 8**은 기능적으로 독립이나 Phase 1 비동기 전환 후가 효율적.

---

## 코드 구조 변경 요약

### `src/connection/rdp.rs` 리팩토링 후 예상 구조

```
rdp.rs
├── connect_and_subscribe()        // 진입점 (변경 없음)
├── async fn run_rdp_worker()      // ★ spawn_blocking → tokio::spawn (Phase 1 완료)
│   ├── connect()                  // ironrdp-tokio + ironrdp-tls 비동기 핸드셸이크 (Phase 1 완료)
│   ├── tokio::select! 메인 루프   // ★ 폴링 → 이벤트 드리븐 (Phase 1 완료)
│   │   ├── input branch           // ironrdp-input 활용 (Phase 3)
│   │   ├── pdu branch             // ActiveStage 출력 처리 + GFX DVC 처리 (Phase 9-B)
│   │   └── shutdown branch        // CancellationToken (Phase 8)
│   └── cleanup                    // graceful shutdown (Phase 8)
├── try_handle_slowpath_bitmap()   // 유지 (IronRDP 한계 보완)
├── 픽셀 변환 함수들               // 유지 (rgb24/bgr24/rgb16/bgrx → RGBA)
├── (제거 완료) tls_upgrade / NoCertificateVerification / extract_tls_server_public_key
│
└── (Phase 9 추가 예정 — ironrdp-egfx 게시 후)
    ├── egfx_handler.rs            // GraphicsPipelineHandler 구현 — FrameUpdate 변환기 (kterm 고유)
    │   ├── on_bitmap_updated()    // BitmapUpdate → FrameUpdate::Rect/Full
    │   ├── on_reset_graphics()    // 버퍼 리셋 + FrameUpdate::Resize
    │   └── on_unhandled_pdu()    // AVC444/ClearCodec/Planar 등 로그 출력
    │   // ★ gfx_processor.rs (DvcProcessor 직접 구현) 불필요 — GraphicsPipelineClient(ironrdp-egfx)로 대체됨
   └── nscodec.rs (선택, 추후)     // IronRDP 공식 NSCodec 지원 후 필요 시 보조 경로 검토
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
| ~~`ironrdp-tokio` API가 `ironrdp-blocking`과 크게 다를 수 있음~~ | ~~Phase 1 지연~~ | ~~IronRDP GitHub 예제 코드 참조, 점진적 마이그레이션~~ | ✅ Phase 1 완료 |
| ~~`ironrdp-tls` rustls feature와 기존 `tokio-rustls` 버전 충돌~~ | ~~Phase 1 빌드 실패~~ | ~~`cargo tree` 의존성 트리 사전 검증~~ | ✅ Phase 1 완료 |
| ~~비동기 전환 중 기존 프레임 배치/스로를 로직 껜짐~~ | ~~Phase 1~~ | ~~기존 타이머 로직을 `tokio::time::interval`로 1:1 이식 후 개선~~ | ✅ Phase 1 완료 |
| NSCodec 협상하지만 디코딩 코드 없음 | 잠재적 화면 깨짐 (서버가 NSCodec 전송 시) | 현재 NSCodec 협상을 비활성화하고, IronRDP 공식 지원 시 구현 예정 | 보류(전략 변경) |
| EGFX GFX 프로세서가 IronRDP에 전용 크레이트 없음 | Phase 9-B-2 전까지 수동 구현 필요 | **Phase 9-B-1**: `ironrdp-dvc` + `ironrdp-pdu` gfx + `ironrdp-graphics::zgfx` 직접 조합으로 Uncompressed 코덱 수동 구현 (즉시 가능). Phase 9-B-2 게시 후 GraphicsPipelineClient로 교체. | Phase 9-B-1 착수 가능 |
| ClearCodec / Planar 코덱 IronRDP에 디코더 없음 | Phase 9-B 일부 코덱 미지원 | MS-RDPEGFX 스펙 직접 구현. 미지원 코덱은 warn 후 skip | Phase 9-B |
| OpenH264 빌드 시 C 컴파일러 필요 | CI/크로스컴파일 환경 빌드 실패 | `source` feature 비활성화 후 시스템 OpenH264 링크 옵션 제공 | Phase 9-C |
| CredSSP 활성화 시 일부 서버와 호환성 문제 | Phase 7 | NLA off 폴백 옵션 유지 | Phase 7 |
| DVC 채널 핸들링이 IronRDP에서 실험적 | Phase 5-6-9 | 채널별 feature gate, 점진적 활성화 | Phase 5+ |

---

## 테스트 전략

| Phase | 테스트 | 상태 |
|-------|--------|------|
| Phase 1 | 비동기 I/O + TLS 전환; 자체서명/공인 인증서 서버; Windows Server 2019/2022 + Win10/11 회귀 테스트 | ✅ 완료 확인 |
| Phase 3 | 한국어 IME 입력, Function 키, 복합 키 조합 | 미완 |
| Phase 4-1 | Windows 텍스트 복사/붙여넣기 양방향 확인 | ✅ 완료 확인 |
| Phase 4-2 | Linux/macOS 텍스트 복사/붙여넣기 양방향 확인 | 설계 단계 |
| Phase 5 | 해상도 변경 후 화면 깨짐 없음 확인 | 미완 |
| Phase 6 | 로컬 파일 원격 열기/저장 | 미완 |
| Phase 7 | NLA 활성 서버 접속, 인증서 검증 경고 표시 | 미완 |
| Phase 8 | 탭 닫기 후 메모리 누수 없음 (sender 명시적 drop 코드 적용 2026-03-26, **실제 확인 추후**), 네트워크 끊김 후 재접속 | 부분 완료 |
| Phase 9-A | NSCodec은 IronRDP 공식 지원 시 반영 예정. 현재는 NSCodec 비협상 상태로 안정성 우선 운영 | 보류 |
| Phase 9-B-1 | EGFX GFX DVC 채널 (Uncompressed) — `GfxProcessor` 구현 완료 2026-03-26, **Win10/11 서버 실제 동작 확인 추후** | 코드 완료, 검증 추후 |
| Phase 9-B-2 | ironrdp-egfx 기반 교체 + AVC420 — ironrdp-egfx crates.io 미게시 | 보류 |
| Phase 9-B (기존) | EGFX GFX 채널 연결 후 Win10/11 서버에서 RemoteFX Progressive / ClearCodec 화면 정상 표시 | 추후 확인 |
| Phase 9-C | H.264 AVC420/AVC444로 동영상 재생 시 화면 정상 출력 및 성능 측정 | 미완 |

- 보안 정책(TLS/NLA/인증서 검증)은 기본 안전 설정을 우선한다.
- 성능 최적화는 full-frame 동작을 먼저 완성한 다음 dirty-rect로 확장한다.
- `russh 0.57+` 계열에서는 `sha1` 프리릴리스 충돌이 재발할 수 있으므로, `russh` 업그레이드는 별도 검증 브랜치에서 수행한다.
