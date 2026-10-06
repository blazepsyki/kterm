# kterm

kterm은 Windows에서 MobaXterm을 대체하는 것을 목표로 하는 Rust 데스크톱 클라이언트입니다. SSH·Telnet·Serial·로컬 셸의 terminal과 RDP·VNC의 원격 화면을 하나의 앱에서 다룹니다.

현재 버전은 **0.1.2**입니다. 기본 연결 경로는 있지만 저장 세션·SFTP·SSH gateway·터널·X11 등 주요 기능이 빠져 있으며, 운영 사용 전에 해결할 신뢰 검증·terminal 안정성 문제가 있습니다.

## 문서 안내

현재 설명과 계획은 다음 3개 문서에서 관리합니다.

| 문서 | 내용 |
|---|---|
| [README.md](README.md) | 프로젝트 목표, 실행 방법, 기능과 주요 제약입니다. |
| [구조와 구현 현황](docs/architecture.md) | 모듈·데이터 흐름, 프로토콜·설정 구현, 확인된 문제, 의존성·라이선스입니다. |
| [개발·검증 계획](docs/development-plan.md) | MobaXterm 기능 격차, 개발 우선순위·완료 조건, 서버 검증, 릴리즈 체크리스트입니다. |

2026-10-06 통합 이전 문서는 [docs/archived/2026-10-06](docs/archived/2026-10-06)에 원본 그대로 보관했습니다. 과거 기록은 백업에서, 현재 지원 여부는 위 문서에서 확인하시면 됩니다. 라이선스 원문은 [docs/license](docs/license)에 유지합니다.

## 실행과 빌드

Windows x64를 현재 배포 대상으로 사용합니다. Rust·Cargo와 MSVC 빌드 도구가 필요합니다. 2026-10-06 분석에서 확인한 도구는 rustc/cargo 1.94.0이며, 최소 지원 버전은 별도로 확정하지 않았습니다.

저장소 루트에서 실행하시면 됩니다.

```powershell
cargo run --locked
```

릴리즈 실행 파일을 빌드하시면 됩니다.

```powershell
cargo build --release --locked
```

실행 파일은 `target/release/kterm.exe`에 생성됩니다. Welcome 탭에서 프로토콜을 선택하고 연결 정보를 입력하시면 됩니다. Local Shell은 시스템에서 탐지한 CMD·PowerShell·Bash 중 하나를 선택합니다. Bash와 Unix 도구는 kterm에 포함되어 있지 않습니다.

## 현재 기능

아래 표는 코드 구현 상태입니다. 서버·장비별 동작 검증 완료를 의미하지는 않습니다.

| 영역 | 구현된 기본 경로 | 남은 제약 |
|---|---|---|
| terminal | ANSI/CSI 일부, 색상, 최대 10,000줄 history, 선택·복사·붙여넣기, IME입니다. | ECH·행 끝 panic·alternate screen·한글 reflow 문제가 확인되었습니다. |
| SSH | 비밀번호 인증, PTY, keepalive, 창 크기 전달입니다. | 서버 키를 무조건 수락하며 개인키·agent·MFA·SFTP·터널이 없습니다. |
| Telnet | 바이트 송수신, codec, NAWS 전송입니다. | 협상 처리가 제한적이며 line ending·local echo 설정이 적용되지 않습니다. 평문 통신입니다. |
| Serial | 비동기 송수신과 data bits·stop bits·parity·hardware flow control입니다. | 실물 검증과 장치 탐색·break·신호 제어가 필요합니다. |
| 로컬 셸 | 실행 파일 탐지와 PTY 입출력·resize입니다. | 사용자 셸 설정 적용과 프로세스 종료 관리가 부족합니다. |
| RDP | TLS/CredSSP, bitmap·RemoteFX, 기본 GFX, 화면·입력, Windows clipboard·오디오 채널입니다. | TLS 신뢰 검증 우회, 제한된 GFX, DisplayControl 미등록, 종료 처리 문제입니다. 오디오는 미검증입니다. |
| VNC | ZRLE·Tight·Raw 협상, CopyRect·커서·view-only, TCP timeout·자동 재시도입니다. | JPEG를 버리며 OS clipboard·SetDesktopSize가 없습니다. 서버별 검증이 필요합니다. |
| UI·설정 | 탭 생성·선택·닫기, 프로토콜 선택, JSON 저장·로드입니다. | 저장 연결 프로필·분할이 없고 일부 설정과 View/Help 메뉴는 실제 기능이 없습니다. |

확인 근거와 재현 결과는 [구조와 구현 현황](docs/architecture.md#확인된-문제)에 정리했습니다. 추가 기능과 순서는 [개발·검증 계획](docs/development-plan.md#개발-우선순위)에서 확인하시면 됩니다.

## 설정과 로그

- 전역 설정은 실행 파일 옆 `kterm_settings.json`에 저장합니다. 기본값과 다른 값만 기록하고 시작 시 로드합니다. 저장 연결 프로필이나 비밀번호 보관 기능은 없습니다.
- 파일이 없거나 읽기·파싱에 실패하면 기본값을 사용합니다. 쓰기 실패는 로그에 기록됩니다. 실행 파일 폴더에 쓰기 권한이 필요합니다.
- 앱 진단 로그는 현재 작업 폴더의 `logs/kterm_YYYYMMDD_HHMMSS.log`에 기록됩니다. 세션별 terminal 출력 저장과는 다른 기능입니다.
- 상세 RDP 추적은 다음과 같이 켤 수 있습니다.

```powershell
$env:KTERM_RDP_TRACE = '1'
cargo run --locked
```

현재 VNC clipboard 내용이 진단 로그에 전달되는 경로와 terminal CSI별 debug 파일 쓰기가 남아 있습니다. [우선 수정 항목](docs/architecture.md#확인된-문제)에 포함되어 있습니다.

## 개발과 검증

```powershell
cargo test --locked
```

2026-10-06 의존성 업그레이드 후 테스트 8개가 통과했습니다. 설정 저장 4건·State 2건·GFX wire 호환성 2건이며 실서버 품질을 보장하지는 않습니다. 최신 버전 적용 예외와 API 수정은 [업그레이드 기록](docs/development-plan.md#의존성-업그레이드-기록), 전체 검증·배포 절차는 [개발·검증 계획](docs/development-plan.md)에 있습니다.

## 라이선스

kterm 자체는 `MIT OR Apache-2.0`입니다. [MIT](docs/license/LICENSE-MIT) 또는 [Apache-2.0](docs/license/LICENSE-APACHE)을 선택하여 적용할 수 있으며 [안내 원문](docs/license/LICENSE)을 함께 제공합니다.

의존성·MPL-2.0 배포 점검과 폰트 고지는 [구조와 구현 현황의 라이선스 절](docs/architecture.md#의존성과-라이선스)에 통합했습니다. D2Coding의 [OFL 원문](assets/fonts/OFL-1.1.txt)과 [폰트 고지](assets/fonts/D2Coding-LICENSE-NOTICE.txt)는 유지합니다.
