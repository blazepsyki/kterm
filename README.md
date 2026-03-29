# kterm

개인 사용 목적에서 시작한 **통합 터미널 클라이언트**입니다.
하나의 GUI 안에서 SSH, Telnet, Serial, 로컬 셸 세션을 탭으로 관리하고, 직접 구현한 터미널 에뮬레이터로 화면을 렌더링합니다.
또한 RDP/VNC 원격 화면 세션을 공용 렌더링 경로로 처리합니다.

## 프로젝트 개요

`kterm`은 "연결(SSH/Telnet/Serial/Local/RDP/VNC)"과 "렌더링(터미널/원격 화면)"을 분리한 구조를 가진 Rust 데스크톱 앱입니다.
UI는 `iced`로 구성되어 있고, 터미널 시퀀스 파싱은 `vte`를 사용합니다.

핵심 목표:

- 여러 종류의 콘솔 접속을 하나의 앱으로 통합
- 탭 기반 세션 전환
- 스크롤백, 텍스트 선택/복사/붙여넣기 등 실사용 기능 제공
- 개인 사용 환경(특히 Windows)에서 빠르게 실행 가능한 클라이언트 구현
- 원격 그래픽 프로토콜(RDP/VNC) 확장을 위한 공통 렌더링 계층 구축

## 주요 기능

- SSH 연결
  - 비밀번호 인증 기반 접속
  - 원격 PTY 요청(`xterm-256color`)
  - 창 크기 변경 시 remote window size 전달
- Telnet 연결
  - Telnet 이벤트를 터미널 출력 바이트로 변환
  - 윈도우 크기 협상(NAWS) 전송
- Serial 연결
  - `tokio-serial` 기반 비동기 Serial 포트 스트림
  - `tokio::io::split`으로 읽기/쓰기 반분할 후 `tokio::select!` 기반 입출력 버퍼링
  - EOF 및 송신자 드롭 시 세션 종료 처리
- 로컬 셸 연결
  - `portable-pty` 기반 로컬 셸 실행
  - 실행 가능 셸 자동 탐지(`pwsh`, `powershell`, `cmd`, `bash`) 및 fallback 제공
- RDP 연결
  - IronRDP 기반 TLS 핸드셰이크 및 `ActiveStage` 루프
  - 빠른 경로(FastPath) PDU와 느린 경로(Slow-path) 비트맵 업데이트 모두 처리
  - 다중 픽셀 포맷 디코딩: RDP6 압축 32bpp, RLE 16/24bpp, 비압축 BGRX/RGB565
  - NSCodec은 현재 비활성화(비협상) 상태이며, IronRDP 공식 지원 시 추후 적용 예정
  - 부분 갱신(Dirty Rect) 기반 프레임 버퍼 + wgpu GPU 텍스처 렌더러
  - 키보드 스캔코드/유니코드 FastPath 매핑
  - XRDP NumLock 불일치 완화: NumPad/Navigation 충돌 키 입력 직전에만 lock-state sync 전송
  - 마우스 이동/클릭/휠 FastPath 매핑
  - 연속 프레임 배치 병합으로 Iced 핸들 재생성 최소화(≈60fps 상한)
  - 클립보드 공유 Phase 4-1 완료: Windows에서 `ironrdp-cliprdr` + `ironrdp-cliprdr-native` 기반 텍스트 복사/붙여넣기 양방향 확인
  - 클립보드 공유 Phase 4-2 예정: Linux/macOS용 `CliprdrBackend` 설계 단계
  - RDP 오디오 재생 채널(`ironrdp-rdpsnd` + cpal 백엔드) 코드 통합 — **테스트 미완료**
- 터미널 UX
  - ANSI/CSI 시퀀스 처리(커서 이동, 지우기, 스크롤, 색/스타일 일부)
  - Wide char(한글 포함) 렌더링 보정 로직
  - 스크롤백 히스토리
  - 마우스 드래그 선택, 복사/붙여넣기
  - IME preedit/commit 처리
- 세션 관리
  - 탭 추가/전환/닫기
  - Welcome 탭에서 연결 타입별 런처 제공(SSH/Telnet/Serial/Local/RDP/VNC)
- VNC 연결
  - `vnc-rs` 기반 비동기 워커(`tokio::spawn`)로 연결/이벤트 루프 처리
  - 인코딩 협상: `CopyRect`, `Raw`, `DesktopSizePseudo`, `CursorPseudo`
  - 이벤트 처리: `SetResolution`, `RawImage`, `Copy`, `SetCursor` -> `FrameUpdate` 변환
  - 입력 처리: `RemoteInput`(키보드 스캔코드/유니코드, 마우스 이동/버튼/수직/수평 휠)
  - Lock 키 동기화: `ConnectionInput::SyncKeyboardIndicators` 반영
  - 주기 `Refresh` + healing `FullRefresh` 경로로 화면 일관성 보정

## 아키텍처

### 1) 앱/UI 레이어

- `src/main.rs`
  - 앱 부트스트랩 및 공용 상수/로깅 초기화
- `src/app/state.rs`, `src/app/model.rs`, `src/app/message.rs`
  - 상태/세션/메시지 모델 정의
- `src/app/update.rs`, `src/app/subscription.rs`
  - 메시지 처리, 세션 라우팅, 이벤트 구독
- `src/ui/view.rs`, `src/ui/settings.rs`
  - 탭/사이드바/커스텀 타이틀바/터미널/원격화면/설정 UI 렌더링
- `src/app/local_shell.rs`
  - 로컬 실행 가능 셸 자동 탐지(`pwsh`, `powershell`, `cmd`, `bash`)

### 2) 터미널 에뮬레이터

- `src/terminal.rs`
  - `TerminalEmulator`: 그리드/히스토리/커서/속성 상태 관리
  - `vte::Perform` 구현으로 제어 시퀀스 처리
  - 캔버스 기반 렌더링(`iced::widget::canvas`)
  - 선택 영역 처리 및 선택 텍스트 추출

### 3) 연결 계층

- `src/connection/mod.rs`
  - 공통 이벤트/입력 타입 정의
  - `ConnectionEvent`: Connected/Data/Frames/Disconnected/Error
  - `ConnectionInput`: Data/Resize/SyncKeyboardIndicators/ReleaseAllModifiers/RemoteInput
- `src/connection/ssh.rs`
  - `russh` 기반 SSH 스트림 구성
- `src/connection/telnet.rs`
  - `nectar` 기반 Telnet codec 처리
- `src/connection/serial.rs`
  - `tokio-serial` 기반 Serial 스트림 처리
- `src/connection/rdp.rs`
  - `ironrdp` 기반 RDP 연결/프레임 처리/입력 이벤트 매핑
- `src/connection/vnc.rs`
  - `vnc-rs` 기반 VNC 연결/프레임 처리/입력 이벤트 매핑
- `src/connection/remote_input_policy.rs`
  - Iced 키 이벤트를 RDP/VNC `RemoteInput`으로 라우팅
- `src/platform/windows.rs`
  - 로컬 PTY 셸(spawn + read/write + resize)
  - Windows CLIPRDR 백엔드 생성/정리 (`create_cliprdr_backend`, `remove_clipboard_for_session`)

### 4) 원격 디스플레이 계층 (RDP/VNC 공용 기반)

- `src/remote_display/mod.rs`
  - `FrameUpdate`(Full/Rect) 타입
  - `RemoteDisplayState`: Arc 기반 Copy-on-Write 프레임 버퍼, Dirty Rect 추적
- `src/remote_display/renderer.rs`
  - `RdpPipeline`: wgpu GPU 텍스처 + WGSL 쉐이더 기반 렌더러
  - 최초 1×1 플레이스홀더 텍스처 → 첫 프레임 수신 시 실제 크기로 교체
  - Dirty Rect 단위 부분 텍스처 업로드로 GPU 대역폭 최소화
- `src/remote_display/rdp_display.wgsl`
  - 뷰포트/텍스처 크기 유니폼 기반 전체 화면 스케일링 렌더링

## 동작 흐름

1. 사용자가 Welcome 화면에서 프로토콜(SSH/Telnet/Serial/Local/RDP/VNC)을 선택
2. 연결 모듈이 비동기 스트림으로 `ConnectionEvent`를 발행
3. `app/update.rs`가 세션 ID 기준으로 이벤트를 해당 탭에 라우팅
4. 터미널 세션은 `TerminalEmulator`에 반영, RDP/VNC 세션은 `RemoteDisplayState`에 프레임 반영
5. `RemoteDisplayState`의 RGBA 버퍼를 wgpu 셰이더 경로로 업로드해 GPU 렌더링
6. 사용자 입력은 프로토콜에 맞게 `ConnectionInput::Data` 또는 `ConnectionInput::RemoteInput`으로 전달

## 빠른 시작

### 요구 사항

- Rust (stable)
- Cargo
- Windows 환경 권장 (현재 로컬 PTY 구현은 Windows 경로 기준)

### 실행

```bash
cargo run
```

### 빌드

```bash
cargo build --release
```

## 프로젝트 구조

```text
src/
  main.rs                 # 앱 부트스트랩 + 공용 상수/로깅
  app/
    mod.rs                # app 모듈 export
    model.rs              # Session/Protocol/Settings 모델
    state.rs              # 전역 상태
    message.rs            # 메시지 enum
    subscription.rs       # 이벤트 구독
    update.rs             # 상태 전이/세션 라우팅
    local_shell.rs        # 로컬 셸 탐지
  ui/
    mod.rs
    view.rs               # 메인 뷰 렌더링
    settings.rs           # 설정 탭 UI
  terminal.rs             # 터미널 에뮬레이터 + 렌더링
  connection/
    mod.rs                # 연결 공통 타입
    ssh.rs                # SSH 연결
    telnet.rs             # Telnet 연결
    serial.rs             # Serial 연결
    rdp.rs                # RDP 연결(IronRDP ActiveStage + 입력 매핑)
    vnc.rs                # VNC 연결(vnc-rs 기반)
    remote_input_policy.rs # RDP/VNC 입력 라우팅 정책
  remote_display/
    mod.rs                # 원격 프레임 상태(Full/Rect + Dirty Rect)
    renderer.rs           # wgpu/WGSL GPU 렌더러(RdpPipeline)
    rdp_display.wgsl      # WGSL 쉐이더 소스
  platform/
    mod.rs
    windows.rs            # 로컬 셸 PTY
assets/
  fonts/
    D2Coding.ttf          # 기본 폰트
```

## 현재 제약 사항

- SSH 서버 호스트 키를 엄격 검증하지 않습니다.
  - 현재 구현은 서버 키 체크에서 `true`를 반환합니다.
- Telnet은 프로토콜 특성상 평문 통신이므로 민감 환경에 부적합합니다.
- 로컬 셸 실행은 `windows.rs`에 구현되어 있어 사실상 Windows 중심입니다.
- RDP 연결은 실사용 가능한 수준으로 구현되어 있으나 다음 항목이 미완입니다.
  - XRDP(LXQt 테스트 환경)는 로그인 화면 → 데스크톱 전환 시 `DeactivateAll`, `SetKeyboardIndicators` 같은 전환 신호를 보내지 않아, NumLock 불일치를 프로토콜 이벤트만으로는 감지할 수 없습니다.
  - 현재는 NumPad/Navigation 충돌 스캔코드(`0x47..0x53`)에 한해 입력 직전 `TS_SYNC_EVENT`를 보내는 절충안을 적용했습니다.
  - 따라서 일반 문자 키에는 추가 sync가 없고, 원격이 별도 신호 없이 lock state를 바꾸는 다른 사례까지 완전하게 해결하지는 못합니다.
  - NSCodec은 현재 비협상 상태로 운영하며, IronRDP 공식 NSCodec 지원이 나오면 그 경로를 우선 적용할 예정입니다.
  - 클립보드 공유는 Phase 4-1 기준으로 Windows에서만 활성화되어 있습니다.
  - Linux/macOS용 클립보드 공유는 Phase 4-2 설계 단계이며 아직 구현되지 않았습니다.
  - 창 리사이즈를 원격 해상도 변경으로 반영하는 기능 미구현
  - 탭 닫기 시 백그라운드 워커/채널 리소스 완전 종료 보장 미완
  - 재접속 UX 및 인증 실패 사유 세분화 미완
  - IME 입력 및 복합 키 조합(예: Ctrl+Alt+Del) 정밀 매핑 미완
  - NLA(CredSSP)는 기본 활성(`enable_credssp: true`)이나 인증서/도메인 정책 세분화는 미완
  - RDP 오디오 재생(`ironrdp-rdpsnd`) 코드 통합 완료 — **실제 동작 테스트 미완료**
- VNC 연결은 기본 사용 가능한 수준이지만 다음 항목은 미완입니다.
  - 클립보드 양방향 연동 미구현
  - 동적 해상도(SetDesktopSize) 미구현
  - 인코딩 확장(Tight/ZRLE) 미구현
  - 자동 재연결/복구 UX 미완
- 일부 고급 이스케이프 시퀀스는 미구현이거나 동작 편차가 있을 수 있습니다.

## 향후 개선 아이디어

- SSH known_hosts 검증 및 키 기반 인증
- 설정 저장(프로파일/최근 접속지)
- RDP 품질 고도화(NLA/CredSSP, IME, 포커스 정책, 재접속)
- RDP 클립보드 Phase 4-2 구현(Linux/macOS용 `CliprdrBackend`, 텍스트 우선)
- RDP 사이즈 변경 연동(원격 해상도 동적 변경)
- VNC 인코딩 확장(Tight/ZRLE) 및 호환성 매트릭스 검증
- VNC 클립보드/자동 재연결/동적 해상도 지원
- 다중 플랫폼 로컬 셸 지원(macOS/Linux)
- 테마/폰트 설정 UI
- 로깅/진단 모드 정리

## 라이선스

이 프로젝트는 다음 듀얼 라이선스를 사용합니다.

- MIT ([docs/license/LICENSE-MIT](docs/license/LICENSE-MIT))
- Apache-2.0 ([docs/license/LICENSE-APACHE](docs/license/LICENSE-APACHE))

원하는 라이선스를 선택해 적용할 수 있습니다.

- 루트 안내 파일: [docs/license/LICENSE](docs/license/LICENSE)
- 소스 파일 상단: `SPDX-License-Identifier: MIT OR Apache-2.0`

## 컴플라이언스 자료

- 제3자 라이선스 목록: [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)
- 릴리즈 점검 체크리스트: [docs/RELEASE_CHECKLIST.md](docs/RELEASE_CHECKLIST.md)
- 폰트 라이선스 원문(OFL 1.1): [assets/fonts/OFL-1.1.txt](assets/fonts/OFL-1.1.txt)
- D2Coding 폰트 고지: [assets/fonts/D2Coding-LICENSE-NOTICE.txt](assets/fonts/D2Coding-LICENSE-NOTICE.txt)
