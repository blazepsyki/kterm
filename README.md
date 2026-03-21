# kterm

개인 사용 목적에서 시작한 **통합 터미널 클라이언트**입니다.
하나의 GUI 안에서 SSH, Telnet, Serial, 로컬 셸 세션을 탭으로 관리하고, 직접 구현한 터미널 에뮬레이터로 화면을 렌더링합니다.
또한 RDP 원격 화면 연결을 위한 초기 그래픽 세션 경로를 포함합니다.

## 프로젝트 개요

`kterm`은 "연결(SSH/Telnet/Serial/Local/RDP)"과 "렌더링(터미널/원격 화면)"을 분리한 구조를 가진 Rust 데스크톱 앱입니다.
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
  - 시리얼 포트 열기/송수신
  - 보레이트 등 기본 설정값 기반 연결
- 로컬 셸 연결
  - `portable-pty` 기반 로컬 셸 실행
  - 실행 가능 셸 자동 탐지(`pwsh`, `powershell`, `cmd`, `bash`) 및 fallback 제공
- RDP 연결 (초기 구현)
  - IronRDP 기반 핸드셰이크 및 세션 시작
  - 원격 프레임(Full/Rect) 수신 후 화면 렌더링
  - 기본 키보드/마우스 입력 전달(FastPath 매핑)
- 터미널 UX
  - ANSI/CSI 시퀀스 처리(커서 이동, 지우기, 스크롤, 색/스타일 일부)
  - Wide char(한글 포함) 렌더링 보정 로직
  - 스크롤백 히스토리
  - 마우스 드래그 선택, 복사/붙여넣기
  - IME preedit/commit 처리
- 세션 관리
  - 탭 추가/전환/닫기
  - Welcome 탭에서 연결 타입별 런처 제공(SSH/Telnet/Serial/Local/RDP)

## 아키텍처

### 1) 앱/UI 레이어

- `src/main.rs`
  - 전역 상태(`State`)와 세션(`Session`) 관리
  - 메시지 기반 업데이트(`Message`, `update`, `subscription`)
  - 탭/사이드바/커스텀 타이틀바/터미널 뷰 렌더링

### 2) 터미널 에뮬레이터

- `src/terminal.rs`
  - `TerminalEmulator`: 그리드/히스토리/커서/속성 상태 관리
  - `vte::Perform` 구현으로 제어 시퀀스 처리
  - 캔버스 기반 렌더링(`iced::widget::canvas`)
  - 선택 영역 처리 및 선택 텍스트 추출

### 3) 연결 계층

- `src/connection/mod.rs`
  - 공통 이벤트/입력 타입 정의
  - `ConnectionEvent`: Connected/Data/Frame/Disconnected/Error
  - `ConnectionInput`: Data/Resize/RdpInput
- `src/connection/ssh.rs`
  - `russh` 기반 SSH 스트림 구성
- `src/connection/telnet.rs`
  - `nectar` 기반 Telnet codec 처리
- `src/connection/serial.rs`
  - `serialport` 기반 Serial 스트림 처리
- `src/connection/rdp.rs`
  - `ironrdp` 기반 RDP 연결/프레임 처리/입력 이벤트 매핑
- `src/platform/windows.rs`
  - 로컬 PTY 셸(spawn + read/write + resize)

### 4) 원격 디스플레이 계층 (RDP/VNC 공용 기반)

- `src/remote_display/mod.rs`
  - `FrameUpdate`(Full/Rect) 타입
  - `RemoteDisplayState` 프레임 버퍼 상태 관리
- `src/remote_display/renderer.rs`
  - Iced 이미지 핸들 생성

## 동작 흐름

1. 사용자가 Welcome 화면에서 프로토콜(SSH/Telnet/Serial/Local/RDP)을 선택
2. 연결 모듈이 비동기 스트림으로 `ConnectionEvent`를 발행
3. `main.rs`가 세션 ID 기준으로 이벤트를 해당 탭에 라우팅
4. 터미널 세션은 `TerminalEmulator`에 반영, RDP 세션은 `RemoteDisplayState`에 프레임 반영
5. 사용자 입력은 프로토콜에 맞게 `ConnectionInput::Data` 또는 `ConnectionInput::RdpInput`으로 전달

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
  main.rs                 # 앱 상태/메시지/레이아웃
  terminal.rs             # 터미널 에뮬레이터 + 렌더링
  connection/
    mod.rs                # 연결 공통 타입
    ssh.rs                # SSH 연결
    telnet.rs             # Telnet 연결
    serial.rs             # Serial 연결
    rdp.rs                # RDP 연결(초기 구현)
  remote_display/
    mod.rs                # 원격 프레임 상태(Full/Rect)
    renderer.rs           # 원격 화면 이미지 렌더러
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
- RDP는 초기 구현 단계입니다.
  - 연결/프레임 표시/기본 입력은 동작하지만, 고급 단축키/IME/포커스 정책/성능 튜닝은 진행 중입니다.
  - 탭 종료/재연결/네트워크 불안정 상황의 안정화 작업이 남아 있습니다.
- VNC는 아직 미구현입니다.
- 일부 고급 이스케이프 시퀀스는 미구현이거나 동작 편차가 있을 수 있습니다.

## 향후 개선 아이디어

- SSH known_hosts 검증 및 키 기반 인증
- 설정 저장(프로파일/최근 접속지)
- RDP 품질 고도화(입력/포커스/성능/복구)
- VNC 연결 백엔드 구현 및 공용 렌더러 연동
- 다중 플랫폼 로컬 셸 지원(macOS/Linux)
- 테마/폰트 설정 UI
- 로깅/진단 모드 정리

## 라이선스

이 프로젝트는 다음 듀얼 라이선스를 사용합니다.

- MIT ([LICENSE-MIT](LICENSE-MIT))
- Apache-2.0 ([LICENSE-APACHE](LICENSE-APACHE))

원하는 라이선스를 선택해 적용할 수 있습니다.

- 루트 안내 파일: [LICENSE](LICENSE)
- 소스 파일 상단: `SPDX-License-Identifier: MIT OR Apache-2.0`

## 컴플라이언스 자료

- 제3자 라이선스 목록: [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)
- 릴리즈 점검 체크리스트: [RELEASE_CHECKLIST.md](RELEASE_CHECKLIST.md)
- CI 라이선스 스캔: [.github/workflows/license-check.yml](.github/workflows/license-check.yml)
- 폰트 라이선스 원문(OFL 1.1): [assets/fonts/OFL-1.1.txt](assets/fonts/OFL-1.1.txt)
- D2Coding 폰트 고지: [assets/fonts/D2Coding-LICENSE-NOTICE.txt](assets/fonts/D2Coding-LICENSE-NOTICE.txt)
