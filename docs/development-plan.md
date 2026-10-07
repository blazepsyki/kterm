# 개발·검증 계획

Windows에서 MobaXterm을 대체하기 위한 기능 격차, 개발 순서, 완료 조건과 배포 절차를 이 문서에서 관리합니다. 현재 코드·문제의 상세 근거는 [구조와 구현 현황](architecture.md), 사용 방법은 [README](../README.md)에 있습니다.

- 정리일: 2026-10-06, 한국 시간 기준입니다.
- 현재 버전: kterm 0.1.2입니다. 분석 출발점은 commit `6f4ec55d3d084746febf14ae2e0815875107c0b0`이며 이후 의존성·API 변경을 반영했습니다.
- 아래 설계·backlog·완료 조건은 **제안**입니다. 이미 구현한 것으로 해석하지 않으셔야 합니다.
- 구현, 실서버 검증, 배포 완료를 각각 구분합니다. 인력·검증 환경이 확정되지 않아 완료 날짜는 추정하지 않았습니다.
- 우선순위 정리에서는 현재 소스·설정·CI와 MobaXterm 공식 공개 자료를 확인했습니다. 이후 K01·K02의 수정·새 검증 결과는 [첫 구현 기록](#k01k02-첫-구현-기록)에서 별도로 관리합니다.

## 목표와 단계

현재는 여러 프로토콜을 연결하는 초기 통합 클라이언트입니다. 일반적인 서버 관리부터 대체하고 Linux GUI·Unix 도구·추가 프로토콜까지 확장하는 중간 목표가 필요합니다. 기능 동등성이 최종 목표이므로 X11과 자체 Unix 실행 환경을 누락 범위로 남겨두어서는 안 됩니다.

비교 범위는 **MobaXterm의 공개 공통 기능과 Professional의 프로그램 기능**입니다. Professional은 세션·터널·매크로 수와 daemon 실행 시간 제한 해제, 설정·로고 사용자화와 기업 배포를 안내합니다. kterm에서도 이 범위를 후속 작업으로 관리합니다. 기술 지원 계약·유료 업데이트 권리는 프로그램 기능 동등성 평가와 구분합니다. [공식 에디션 비교](https://mobaxterm.mobatek.net/download.html)

대체 여부는 기능 이름이나 연결 성공만으로 판정하지 않습니다. 같은 Windows PC·서버에서 연결 → 인증 → 작업 → 저장·전송 → 장애 복구 → 종료까지 비교하고, 기능별 결과를 구현·검증·배포 상태로 기록합니다. B단계는 일상 서버 관리의 중간 목표이며, 최종 동등성은 D·E단계와 남은 비교 항목까지 검증한 뒤 판단합니다.

| 단계 | 사용자에게 제공할 결과 | 다음 단계로 넘어갈 조건 |
|---|---|---|
| A: 기본 신뢰·안정성 | 안전하게 연결하고 정상적으로 종료할 수 있습니다. | K01~K07과 terminal 재현 오류가 해결되고 회귀 검증이 있습니다. |
| B: 서버 관리 대체 | 저장 세션으로 key/MFA 로그인하고 파일을 옮기며 bastion·터널을 사용합니다. | profile·SSH 인증·SFTP·gateway·검색·출력 저장의 실제 업무 시나리오가 통과합니다. |
| C: 통합 작업 환경 | 분할 terminal과 검증된 RDP/VNC·원격 편집을 사용합니다. | 서버 matrix, clipboard·입력·resize·장애·성능 결과가 있습니다. |
| D: Linux GUI·Unix 환경 | X11 앱과 로컬 Unix 도구를 kterm에서 사용합니다. | X server·forwarding·도구 환경의 설치/portable 동작과 배포 조건을 만족합니다. |
| E: 전체 범위 확대 | 매크로·동시 실행·추가 프로토콜·내장 도구·기업 배포를 제공합니다. | 비교 항목별 미구현·미검증을 해소하고 Professional의 제한 없는 사용·사전 설정 배포를 검증합니다. |

현재 A단계의 선행 문제가 남아 있습니다. B단계에는 저장 profile·고급 SSH·파일 전송·gateway 모듈을 새로 추가해야 하며 D·E단계에는 runtime·protocol·배포 과제가 포함됩니다.

## MobaXterm 기능 격차

2026-10-06 분석에서 확인한 [공식 기능 안내](https://mobaxterm.mobatek.net/features.html), [사용 문서](https://mobaxterm.mobatek.net/documentation.html), [에디션 비교](https://mobaxterm.mobatek.net/download.html)를 기준으로 했습니다. MobaXterm을 같은 서버에서 비교 실행하지는 않았습니다.

아래 44개는 공개 기능을 작업 단위로 묶은 분석 목록이며 모든 옵션을 개별 집계한 것은 아닙니다. 기존 36개에 추가 프로토콜·terminal 강조·시작 자동화·기업 배포 등의 누락 비교 항목을 추가했습니다. **기본 구현 3개, 부분 구현 13개, 미구현 28개**입니다. 서로 다른 규모의 항목을 세었으므로 개발 완료율·품질 점수가 아닙니다.

- 기본 구현: 표에 적힌 좁은 기본 동작의 경로가 있습니다. 제품 전체와 동등하다는 의미는 아닙니다.
- 부분 구현: 일부 코드·UI가 있으나 필수 동작·검증·호환성이 부족합니다.
- 미구현: 현재 제품에 통합된 대응 기능이 없습니다. 외부 CLI 실행 가능 여부와 구분합니다.

| 번호 | 비교 항목 | 판정 | 현재 상태와 근거 |
|---:|---|---|---|
| 01 | 다중 탭 생성·선택·닫기 | 기본 구현 | `Session`, `TabSelected`, `CloseTab`이 있습니다. 순서 변경·복제·창 분리는 없습니다. [model](../src/app/model.rs), [update](../src/app/update.rs) |
| 02 | 기본 terminal 출력·스크롤·색상 | 부분 구현 | CSI·SGR, 최대 10,000줄 history가 있습니다. K02에서 ECH와 경계 panic 회귀를 수정했으며 실제 TUI·장시간 검증은 남아 있습니다. [terminal](../src/terminal.rs) |
| 03 | TUI·terminal mode 호환성 | 부분 구현 | 일부 CSI만 처리하며 alternate screen·DEC private mode·application cursor mode 등이 빠져 있습니다. [terminal](../src/terminal.rs) |
| 04 | 한글·IME·문자폭·reflow | 부분 구현 | IME preedit/commit과 문자폭 처리가 있습니다. K02에서 continuation cell 중복 계산과 resize cursor 회귀를 수정했습니다. grapheme 단위 저장과 GUI 검증은 남아 있습니다. [terminal](../src/terminal.rs), [subscription](../src/app/subscription.rs) |
| 05 | 기본 선택·복사·붙여넣기 | 기본 구현 | 선택과 clipboard 읽기·쓰기가 연결되어 있습니다. 다중 행 확인·붙여넣기 지연·bracketed paste는 없습니다. [update](../src/app/update.rs) |
| 06 | terminal 검색·URL 열기 | 미구현 | 검색 상태·메시지와 링크 탐지·실행 경로가 없습니다. |
| 07 | 폰트·색상·인코딩 사용자 설정 | 미구현 | D2Coding과 셀 크기가 고정되어 있습니다. Theme의 폰트·색상은 readonly placeholder입니다. [settings](../src/ui/settings.rs) |
| 08 | 세션별 terminal 출력 저장 | 미구현 | 앱 진단 로그는 있으나 terminal 출력 스트림을 세션별 저장하는 기능은 없습니다. [main](../src/main.rs), [update](../src/app/update.rs) |
| 09 | 화면 분할·탭 분리 창 | 미구현 | 한 active session을 표시하며 pane tree·다중 창 모델이 없습니다. [view](../src/ui/view.rs) |
| 10 | 여러 terminal 동시 입력 | 미구현 | 입력이 active session에만 전송됩니다. [update](../src/app/update.rs) |
| 11 | 매크로·명령 snippet | 미구현 | 기록·저장·재생 모델과 UI가 없습니다. |
| 12 | 저장 세션·폴더·최근 접속 | 미구현 | sidebar는 새 SSH 버튼뿐이며 `Session`은 실행 중 상태입니다. JSON에는 연결 프로필이 없습니다. [view](../src/ui/view.rs), [settings_persistence](../src/app/settings_persistence.rs) |
| 13 | 세션 가져오기·내보내기·공유 | 미구현 | 프로필 schema와 import/export 경로가 없습니다. |
| 14 | 전역 설정 저장·복원 | 기본 구현 | 실행 파일 옆 JSON 저장·로드와 테스트가 있습니다. 모든 설정의 실제 적용을 뜻하지는 않습니다. [settings_persistence](../src/app/settings_persistence.rs) |
| 15 | SSH 비밀번호 연결·PTY | 부분 구현 | password 인증, shell, keepalive, resize가 있습니다. 서버 키 신뢰와 종료 처리가 부족합니다. [ssh](../src/connection/ssh.rs) |
| 16 | SSH 개인키·passphrase·MFA | 미구현 | `authenticate_password`만 사용합니다. 개인키·keyboard-interactive UI가 없습니다. [ssh](../src/connection/ssh.rs) |
| 17 | SSH agent 인증·forwarding | 미구현 | forwarding 설정은 저장되지만 연결에서 읽지 않습니다. agent 인증 구현도 없습니다. |
| 18 | SSH known_hosts·자격 증명 보관 | 미구현 | 서버 키를 항상 허용하고 credential vault는 없습니다. 비밀번호 마스킹은 있습니다. [ssh](../src/connection/ssh.rs), [state](../src/app/state.rs) |
| 19 | SSH gateway·proxy 연결 | 미구현 | 프로토콜별 TCP 직접 연결입니다. gateway 경유 transport가 없습니다. |
| 20 | SSH local·remote·dynamic 터널 | 미구현 | forwarding listener, tunnel 모델·관리 UI가 없습니다. |
| 21 | SFTP/SCP 파일 탐색·전송 | 미구현 | 파일 목록·전송 queue·SFTP subsystem 경로가 없습니다. |
| 22 | 원격 파일 편집·저장 | 미구현 | 파일 다운로드·편집기 연동·업로드·충돌 처리 경로가 없습니다. |
| 23 | X11 서버·SSH X11 forwarding | 미구현 | 로컬 X server, X11 channel 처리, cookie·DISPLAY 관리가 없습니다. |
| 24 | XDMCP 데스크톱 | 미구현 | 프로토콜 모델과 X server 기반이 없습니다. |
| 25 | 자체 Unix 도구·패키지 환경 | 미구현 | Bash 실행 파일을 탐지할 뿐 Unix 도구나 package runtime을 배포하지 않습니다. [local_shell](../src/app/local_shell.rs) |
| 26 | 로컬 셸·외부 Unix 환경 연결 | 부분 구현 | CMD·PowerShell·PATH의 Bash와 PTY가 있습니다. 셸 설정 적용·WSL 배포판 UI·종료 관리가 부족합니다. [local_shell](../src/app/local_shell.rs), [windows](../src/platform/windows.rs) |
| 27 | Telnet | 부분 구현 | codec와 NAWS 전송이 있으나 option 이벤트를 무시하며 line ending·local echo 설정을 적용하지 않습니다. [telnet](../src/connection/telnet.rs) |
| 28 | Serial | 부분 구현 | 기본 송수신과 포트 설정은 있습니다. 장치 선택·break·신호 제어가 없고 실물 테스트는 기존 체크리스트에서 미완입니다. [serial](../src/connection/serial.rs), [과거 검증 기록](archived/2026-10-06/docs/task.md) |
| 29 | RDP 화면·키보드·마우스 | 부분 구현 | IronRDP와 bitmap·RemoteFX·공용 renderer가 있습니다. GFX의 일부 codec·명령은 무시합니다. [rdp](../src/connection/rdp.rs) |
| 30 | RDP NLA·도메인·인증서 정책 | 부분 구현 | CredSSP 옵션은 연결됩니다. domain은 `None`이며 TLS 신뢰 검증을 우회합니다. [rdp](../src/connection/rdp.rs), [SEC-02](architecture.md#확인된-문제) |
| 31 | RDP clipboard·오디오 | 부분 구현 | Windows clipboard와 rdpsnd 코드가 있습니다. 텍스트 clipboard는 기존 검증 기록이 있고 오디오는 미검증입니다. [windows](../src/platform/windows.rs), [README](../README.md) |
| 32 | RDP 드라이브·프린터·포트·smartcard | 미구현 | 해당 리디렉션 채널과 사용자 설정이 없습니다. |
| 33 | RDP 동적 크기·화면 구성 | 부분 구현 | resize 메시지·`encode_resize` 호출은 있지만 DisplayControlClient 등록이 빠져 있습니다. 다중 모니터 UI도 없습니다. [rdp](../src/connection/rdp.rs), [RDP-01](architecture.md#확인된-문제) |
| 34 | VNC 화면·입력·clipboard·크기 | 부분 구현 | Raw·ZRLE·Tight 협상, CopyRect·커서·view-only가 있습니다. JPEG를 버리고 OS clipboard·SetDesktopSize가 없습니다. [vnc](../src/connection/vnc.rs) |
| 35 | 재접속·timeout·취소·종료 | 부분 구현 | VNC 재시도와 TCP timeout은 있습니다. 다른 프로토콜 정책과 연결 중 취소·종료는 부족합니다. [update](../src/app/update.rs), [connection](../src/connection/mod.rs) |
| 36 | 내장 서버·관리 도구·plugin | 미구현 | daemon 관리, 파일 비교·네트워크 도구·plugin 실행 환경이 없습니다. |
| 37 | FTP·FTPS 독립 세션 | 미구현 | `ProtocolMode`와 connection 모듈에 FTP/FTPS가 없습니다. [model](../src/app/model.rs), [connection](../src/connection/mod.rs) |
| 38 | Mosh 세션 | 미구현 | 프로토콜 선택·SSH bootstrap·UDP 세션 경로가 없습니다. |
| 39 | Rlogin·Rsh 연결 | 미구현 | 해당 연결 backend와 세션 설정이 없습니다. |
| 40 | terminal 키워드·구문 강조 | 미구현 | SGR 색상 처리는 있지만 사용자 강조 규칙·규칙 편집·저장 모델은 없습니다. [terminal](../src/terminal.rs) |
| 41 | CLI·바로가기에서 저장 세션 실행 | 미구현 | 저장 profile과 이를 선택하는 CLI 인자 처리 경로가 없습니다. [main](../src/main.rs) |
| 42 | 세션 그룹 실행·시작 명령·지속 home | 미구현 | 그룹 실행과 시작 스크립트·home 관리가 제품에 통합되어 있지 않습니다. Bash 탐지와 구분합니다. |
| 43 | 사용자 키보드 단축키 설정 | 미구현 | 키 이벤트 처리는 있지만 사용자 keymap 편집·저장·충돌 검증 경로는 없습니다. [subscription](../src/app/subscription.rs) |
| 44 | Professional 설정 사용자화·기업 배포 | 미구현 | 사전 설정 bundle·정책·로고 사용자화·installer가 없습니다. 현재 태그 release는 실행 파일을 빌드·업로드합니다. [.github/workflows/release.yml](../.github/workflows/release.yml) |

추가 항목의 비교 근거는 [공식 기능 안내](https://mobaxterm.mobatek.net/features.html)의 구문 강조·Customizer, [사용 문서](https://mobaxterm.mobatek.net/documentation.html)의 세션 종류·그룹 실행·CLI·단축키·지속 home, [에디션 비교](https://mobaxterm.mobatek.net/download.html)의 기업 배포입니다. 내장 서버·도구·plugin은 36번의 큰 묶음이므로 K29에서 개별 목록을 확정하기 전까지 전체 범위가 조사 완료되었다고 판단하지 않습니다.

비밀번호 기반 단순 명령은 제한적으로 평가할 수 있지만 운영 서버 관리는 신뢰 검증·key/MFA·SFTP·profile 부재로 어렵습니다. bastion 뒤 접속과 Linux GUI·자체 Unix CLI 환경은 제품 내부에서 대체할 수 없습니다. RDP/VNC와 Serial은 서버·장비별 평가가 필요합니다.

## 개발 우선순위

우선순위는 다음 기준으로 정했습니다. 순서는 **설계 제안**이며 사용 빈도·개발 기간을 실측한 순위가 아닙니다.

- **P0:** 신뢰 검증 우회, crash, 종료 실패, 로그 노출, 화면·입력 누락 등 기존 기능의 정확도를 먼저 수정합니다. 회귀 검증도 이 단계부터 추가합니다.
- **P1:** 저장 세션·SSH 인증·파일 전송·gateway 등 일상 서버 관리에 필요한 기능과 기존 프로토콜의 기본 사용 경로를 완성합니다.
- **P2:** X11·Unix 환경, 분할·원격 편집·자동화·추가 프로토콜을 구현합니다. X11·Unix 환경의 배포 가능성 조사는 P1부터 시작합니다.
- **P3:** 기업 사용자화·내장 서버·전문 도구·plugin 등 나머지 동등성 범위를 완성합니다. 후순위는 최종 목표에서 제외한다는 뜻이 아닙니다.

표의 ID는 작업 추적용입니다. 같은 우선순위 안에서는 번호순으로 착수하되 선행 조건을 충족한 독립 작업은 별도로 진행할 수 있습니다. S/M/L/XL은 상대적인 범위이며 작업일 추정이 아닙니다. 경로는 `src/` 기준이며 새 모듈 이름은 제안입니다.

| ID | 우선순위·유형 | 작업 | 관련 위치·선행 조건 | 범위 | 완료 조건 |
|---:|---|---|---|---|---|
| K01 | P0·검증 추가 | terminal 회귀·Windows PR CI | `terminal.rs`, `.github/workflows` | M | TERM-01~04를 고정 입력으로 검증하고, PR마다 check/test와 기존 warning을 구분한 lint가 실행됩니다. |
| K02 | P0·수정 | 행 끝 panic·ECH·한글 reflow·로그 노출 | `terminal.rs`, `connection/vnc.rs`, `app/update.rs`; K01 | S~M | TERM-01·02·04가 통과하고 clipboard 본문·CSI별 무조건 파일 쓰기가 제거됩니다. |
| K03 | P0·수정·추가 | SSH known_hosts·RDP TLS 정책 | `connection/ssh.rs`, `connection/rdp.rs`, TLS adapter | M~L | 신규 키 확인·일치·변경 거부와 인증서 chain/hostname/signature·사설 CA 정책을 검증합니다. |
| K04 | P0·수정 | 연결 전 취소·종료·오류 상태 | `app/model.rs`, `app/update.rs`, 모든 worker·PTY | M~L | 연결 시작부터 cancel handle이 있고 인증·handshake·retry 중 닫기와 EOF에서 task/socket/child 종료를 확인합니다. |
| K05 | P0·수정·추가 | terminal mode·입력·붙여넣기 | `terminal.rs`, `app/subscription.rs`; K01·K02 | L | TERM-03, vim/less/top/tmux, alternate screen·cursor/keypad mode·Ctrl/Alt/F-key·bracketed paste·IME가 동작합니다. |
| K06 | P0·수정 | 원격 화면 누락·입력 라우팅 | `connection/rdp.rs`, `connection/vnc.rs`, `app/subscription.rs`; K04 | M~L | 미처리 GFX/JPEG를 협상에서 제외하거나 처리하고 viewport 밖 입력·focus 이동 후 눌린 키가 남지 않는지 검증합니다. 제한은 UI에 표시합니다. |
| K07 | P0·수정 | 설정 적용·저장 실패 처리 | `ui/settings.rs`, `app/update.rs`, `app/settings_persistence.rs` | M | 미구현 옵션을 비활성화·명시하고 숫자 검증·atomic save·쓰기 실패 안내·설치/portable 저장 위치를 검증합니다. |
| K08 | P1·추가 | 저장 profile·폴더·최근 접속 | 새 `app/profile_persistence.rs`, `app/model.rs`, `ui/view.rs`; K04·K07 | M | 전역 설정과 실행 Session을 분리하고 profile CRUD·복제·검색·재접속·schema 복원을 검증합니다. 비밀번호는 저장하지 않습니다. |
| K09 | P1·추가 | SSH 개인키·passphrase·agent·MFA | `connection/ssh.rs`, 인증 UI; K03·K04 | M~L | 암호화 key·agent·여러 keyboard-interactive challenge·실패·취소가 동작합니다. agent forwarding은 별도 선택으로 둡니다. |
| K10 | P1·추가 | 자격 증명 보관·profile 이전 | 새 credential 모듈, profile persistence; K08·K09 | M | OS vault 참조·삭제·재입력과 비밀값 없는 export/import를 검증합니다. MobaXterm 세션·폴더 import는 샘플로 검증하고 미지원 옵션을 보고합니다. |
| K11 | P1·추가 | SSH service·SFTP browser·전송 | 새 SSH service·SFTP 모듈; K03·K04·K08·K09 | L | SSH 탭 연동·독립 SFTP, drag/drop·목록·업/다운로드·취소·진행률·충돌·권한·symlink·단절 후 파일 상태를 검증합니다. |
| K12 | P1·추가 | SSH gateway·proxy transport | 새 transport 모듈, SSH service; K09·K11의 channel 소유 경계 | L | gateway·대상 서버 각각을 인증·신뢰 검증하고 SSH/SFTP/Telnet/RDP/VNC 경유 연결·단절·취소를 검증합니다. |
| K13 | P1·추가 | local·remote·dynamic 터널 | 새 tunnel 모델·관리 UI; K04·K08·K09, gateway 조합은 K12 | L | listener·SOCKS·포트 충돌·종료·재시작·profile 저장을 검증하고 shell 없이도 터널을 관리합니다. |
| K14 | P1·추가 | 검색·출력 저장·URL·폰트·인코딩 | `terminal.rs`, profile 설정; K05·K08 | M~L | history 검색·UTF-8/선택 인코딩·세션별 출력 파일·URL 열기·폰트/크기/색상·다중 행 paste 확인이 동작합니다. |
| K15 | P1·수정·추가 | RDP/VNC 기본 업무 완성 | 두 connection·remote_display; K03·K04·K06 | L | RDP domain·DisplayControl·audio, VNC OS clipboard·SetDesktopSize·전체 handshake timeout을 구현하고 서버 matrix를 검증합니다. |
| K16 | P1·수정·추가 | Telnet·Serial·로컬 셸 완성 | 해당 connection·`app/local_shell.rs`·`platform/windows.rs`; K04·K05·K07 | M~L | Telnet 협상/line ending/echo, Serial 탐색/break/신호/분리, 셸 인자·login mode·WSL 선택·kill/wait를 실제 환경에서 검증합니다. |
| K17 | P1·수정·검증 | queue·성능·배포 smoke | 공통 connection·renderer·release workflow; K04·K06·K07 | M~L | bounded queue와 frame 순서를 검증하고 대량 출력·다중 세션·고해상도 측정, 고지 포함 artifact 실행 결과를 기록합니다. |
| K18 | P1·조사 | X server·Unix runtime 배포 결정 | X11/도구 후보·license·portable 요구 | M | Windows 지원·재배포 조건·용량·설치 권한·업데이트·외부 설치 의존성을 비교하고 K21·K22의 구현 대상을 결정합니다. |
| K19 | P2·추가 | 분할·탭 이동·창 분리 | pane tree·viewport·session ID; K04·K05·K06·K08 | L | pane별 focus/IME/resize/clipboard·분리/재결합·layout 복원이 동작합니다. |
| K20 | P2·추가 | 원격 편집·SCP 호환 경로 | K11 | M | 외부/내장 편집기 저장·재업로드·원격 충돌·실패 시 원본 보존과 SCP 전용 대상의 제한을 검증합니다. |
| K21 | P2·추가 | X11 server 관리·SSH forwarding | K03·K04·K09·K18 | XL | cookie·DISPLAY·channel·server 수명주기, xterm/GUI·clipboard·한글 입력·OpenGL 지원 범위를 검증합니다. |
| K22 | P2·추가 | Unix 도구·지속 home·package 환경 | local profile·K16·K18 | L~XL | bash/grep/awk/sed/rsync·도구 설치/업데이트·지속 home을 검증합니다. WSL 연동과 외부 설치 없는 portable 배포는 각각 통과해야 합니다. |
| K23 | P2·추가 | 매크로·snippet·동시 입력 | K08·K19 | M~L | 기록·저장·재생·대상 표시·중지·실패와 비밀 입력 제외를 검증합니다. 세션/터널/매크로 개수에 제품상의 제한을 두지 않습니다. |
| K24 | P2·추가 | 단축키·강조 규칙·시작 자동화 | K05·K08·K14, home/script는 K22 | M | 사용자 keymap·충돌 안내·강조 규칙·CLI/바로가기·그룹 실행·시작 명령을 검증합니다. |
| K25 | P2·추가 | FTP/FTPS·Mosh·Rlogin/Rsh | K08·K12·K16, 프로토콜별 backend | L~XL | 독립 세션·인증·취소·재접속을 검증하고 FTPS 인증서/전송 모드와 Mosh UDP 단절 복구·legacy 경로를 각각 기록합니다. |
| K26 | P2·추가 | XDMCP | K18·K21 | L | X server를 재사용하고 원격 Unix desktop의 접속·화면·입력·종료를 검증합니다. |
| K27 | P2·추가 | RDP 리디렉션·다중 모니터 | K15 | L~XL | drive·printer·port·smartcard·파일/이미지 clipboard·monitor별 채널 협상·권한·장애를 검증합니다. |
| K28 | P3·추가 | 기업 배포·사전 설정·사용자화 | K08·K10·K17 | L | 설정·로고 bundle, shared profile·정책·installer·업데이트/rollback을 검증합니다. daemon 실행 시간에도 제품상의 제한을 두지 않습니다. |
| K29 | P3·추가 | 내장 서버·도구·plugin | K18·K22·K28, 세부 기능 inventory | XL | 공식 목록의 서버·편집기·파일 비교·네트워크/키 관리 도구·plugin을 개별 작업으로 나누고 실행·중지·업데이트·배포를 검증합니다. |

우선 수정의 근거는 [확인된 문제](architecture.md#확인된-문제)의 SEC·TERM·LIFE·RDP·VNC·CFG·PERF·INPUT 항목입니다. K06의 우선순위를 기존 P1에서 P0로 올린 이유는 화면 누락·잘못된 입력이 기존 연결 기능의 정확도 문제이기 때문입니다. 실제 발생 빈도·성능 영향은 미측정입니다. CI를 마지막에 추가하지 않고 K01부터 각 수정과 함께 검증합니다.

K18은 다른 P1 기능의 완료를 기다리지 않고 P1 초반부터 조사합니다. K01·K17의 검증은 이후 모든 작업에 계속 적용합니다. 현재 의존성의 russh·picky-krb pin은 [업그레이드 제약](#최신-버전-적용-예외)을 유지하며, 신규 SSH 기능에 필요한 API가 없으면 해당 작업에서 호환성을 재검토합니다. 업그레이드 자체를 기능 구현 완료로 처리하지 않습니다.

### 기능 비교표와 작업의 대응

이 대응표는 완료 결과를 어디에 연결할지 정한 것입니다. 현재 완료 상태를 나타내지는 않습니다.

| 비교 번호 | 담당 작업 |
|---|---|
| 01·09 | K08·K19·K24 탭·profile·pane·그룹 실행입니다. |
| 02·03·04·05 | K01·K02·K05 terminal 정확도·mode·입력과 K14 붙여넣기 정책입니다. |
| 06·07·08 | K14 검색·URL·폰트·인코딩·출력 저장입니다. |
| 10·11 | K23 동시 입력·snippet·매크로입니다. |
| 12·13·14 | K07 설정 저장, K08 profile, K10 이전·공유 형식입니다. |
| 15·16·17·18 | K03 SSH 신뢰, K04 수명주기, K09 인증·forwarding 선택, K10 vault입니다. |
| 19·20 | K12 gateway·proxy, K13 터널입니다. |
| 21·22 | K11 SFTP와 K20 원격 편집·SCP입니다. |
| 23·24·25·26 | K16 셸·WSL, K18 조사, K21 X11, K22 Unix 환경, K26 XDMCP입니다. |
| 27·28 | K16 Telnet·Serial입니다. |
| 29·30·31·32·33·34 | K03 RDP 신뢰, K06 화면·입력, K15 RDP/VNC 기본 작업, K27 리디렉션·monitor입니다. |
| 35 | K04 취소·종료·공통 복구, K15 프로토콜별 보완입니다. |
| 36 | K29 내장 서버·도구·plugin이며 K22 runtime을 재사용합니다. |
| 37·38·39 | K25 추가 프로토콜입니다. |
| 40·41·42·43 | K24 강조·CLI·그룹·시작 자동화·단축키, K22 지속 home입니다. |
| 44 | K28 기업 배포·사용자화이며 K23·K28에서 Professional의 수·시간 제한 해제를 관리합니다. |

36번의 세부 조사에서는 [사용 문서](https://mobaxterm.mobatek.net/documentation.html)에 있는 TFTP·HTTP·FTP·SSH/SFTP·Telnet 서버와 편집기·이미지 뷰어·파일 비교·포트 분석·packet capture를 우선 목록화합니다. [에디션 비교](https://mobaxterm.mobatek.net/download.html)에 언급된 NFS·Cron과 plugin별 도구도 조사 대상에 포함합니다. 현재 공개 페이지에서 확인하지 못한 세부 옵션은 조사 필요로 기록합니다.

### 바로 착수할 수정 범위

첫 변경은 **K01·K02의 재현 가능한 작은 수정**으로 시작하시는 것을 권장합니다. 다음 범위를 분리해서 review할 수 있게 진행합니다.

1. 기존 TERM-01~04 입력을 자동 회귀로 옮깁니다. 실패를 확인한 뒤 TERM-01·02·04를 각각 수정하고 TERM-03은 K05에서 구현합니다. 중간 CI에는 미해결 항목을 명시하며 전부 통과했다고 기록하지 않습니다.
2. `clear_line`의 cursor 경계와 wrap pending을 정리하고, ECH가 문자를 당기지 않도록 고칩니다. reflow에서 wide cell과 continuation cell을 중복 계산하지 않도록 수정합니다. ConPTY 보정은 공통 ANSI 의미를 바꾸지 않도록 backend별로 분리합니다.
3. VNC clipboard를 진단용 `Data` 문자열로 전달하는 경로를 제거하고, clipboard 전용 event는 K15에서 연결합니다. CSI별 파일 쓰기는 기본 해제하고 필요한 진단만 선택해서 기록합니다.
4. Windows PR check/test를 추가하고 각 수정의 회귀 결과를 확인합니다. 이어 K03의 신뢰 UI와 K04의 연결 시작 시 cancel handle·종료 상태를 구현합니다.

이 범위는 저장 세션·SFTP·X11을 한 변경에 넣지 않습니다. 이미 재현된 오류부터 고쳐 공통 terminal을 사용하는 SSH/Telnet/Serial/Local의 기반을 확보한다는 판단입니다. terminal core를 교체할지는 K05에서 호환성·라이선스·변경 범위를 비교해 결정하며, 기존 설계의 부분 수정으로 해결할 수 있는 경계 오류는 먼저 처리합니다.

### K01·K02 첫 구현 기록

2026-10-06, Windows x64의 현재 작업 트리에서 구현·검증했습니다. commit·릴리즈·GitHub CI 실행 완료를 의미하지 않습니다.

| 작업 | 반영한 내용 | 현재 상태 |
|---|---|---|
| K01 | TERM-01~04를 자동 회귀로 옮기고 행 끝·wide cell·resize·cursor 보완 테스트를 추가했습니다. [.github/workflows/windows-ci.yml](../.github/workflows/windows-ci.yml)에 Windows PR check/test·선별 Clippy를 추가했습니다. | 로컬 검증은 통과했습니다. GitHub CI 실행은 미확인입니다. TERM-03은 K05 사유를 명시해 ignored로 유지합니다. |
| K02 terminal | `wrap_pending` 분리·ECH/EL 범위 삭제·wide continuation 소비·resize cursor 재배치·0 크기 보정·1열 wide glyph 대체를 구현했습니다. | TERM-01·02·04와 경계·resize 회귀가 통과했습니다. 전체 TUI·IME·ConPTY 실기는 미검증입니다. |
| K02 로그 | VNC clipboard 본문의 Data 전달과 CSI별 파일 쓰기를 제거했습니다. VNC 수신 로그는 byte 길이만 기록하며 본문·개행 제외를 테스트했습니다. | 로그 회귀는 통과했습니다. OS clipboard 기능 추가는 K15입니다. |
| ConPTY 범위 | 기존 보정은 Windows Local 생성 시에만 활성화하고 다른 terminal은 기본 비활성으로 둡니다. | 원격 CUP/ECH가 다른 wrapped 행을 지우지 않는 회귀가 통과했습니다. Local 보정 자체는 후속 실기 검증이 필요합니다. |

| 실행한 검증 | 결과 |
|---|---|
| 수정 전 기존 테스트 | `cargo test --locked --offline`: 8 passed였습니다. |
| 수정 전 신규 재현 | TERM-01·02·04 실패를 확인했습니다. TERM-03은 별도 구현 대상으로 표시했습니다. |
| 수정 후 테스트 | `cargo test --locked --offline`: 21 passed·0 failed·1 ignored입니다. |
| 전체 target 컴파일 | `cargo check --locked --offline --all-targets`: 통과했습니다. 기존 dead_code 경고는 남아 있습니다. |
| 선별 lint | `cargo clippy --locked --offline --all-targets -- -D clippy::correctness -D unused_imports -D unused_variables`: 통과했습니다. 기존 style·복잡도 등 Clippy 경고는 남아 있습니다. |
| 릴리즈 빌드 | `cargo build --release --locked --offline`: Windows x64 빌드가 통과했습니다. 실행 파일의 GUI·실서버 smoke는 수행하지 않았습니다. |
| 전체 formatting | `cargo fmt --all -- --check`: 실패했습니다. 기존 formatting 차이와 Telnet/PTY 파일의 trailing whitespace 오류가 있으며, 이번 변경에서 전체 소스를 일괄 재포맷하지 않았습니다. formatting을 새 CI의 완료 조건으로 추가하지 않았습니다. |

자동 테스트가 통과한 재현과 실서버·GUI 검증을 구분합니다. 다음 착수 대상은 K03의 SSH known_hosts·RDP TLS 정책이며, 연결 전 신뢰 확인을 취소할 수 있도록 K04와 입력·상태 계약을 맞추어야 합니다. K05의 alternate screen과 전체 terminal mode는 아직 구현하지 않았습니다.

### 신규 구현의 선행 관계

| 구현 흐름 | 먼저 정할 계약 | 재작업을 줄이는 이유·판단 |
|---|---|---|
| K03·K04 → K08·K09 → K11 → K12·K13 | 신뢰 요청/응답·취소·profile ID·SSH channel 소유권·transport | SFTP·터널·gateway가 각각 인증과 종료를 다시 구현하지 않도록 합니다. K11을 시작할 때 gateway를 받을 transport 경계도 함께 설계합니다. |
| K08·K09 → K10 → K28 | schema version·credential 참조·비밀값 없는 export·공유 profile | 비밀번호를 JSON에 저장한 뒤 vault로 옮기는 설계를 피하고 이전·기업 배포에서 같은 profile을 재사용합니다. |
| K05·K06·K08 → K19 → K23 | session ID와 pane ID 분리·viewport·focus·입력 대상 | 고정 offset과 active session 전용 입력을 유지한 채 분할·동시 입력을 추가하면 입력 오류가 확대될 수 있습니다. |
| K18 → K21·K22 → K26·K29 | 외부 바이너리 수명주기·배포 license·runtime 위치·업데이트 | X11·Unix 환경은 구현 외에도 배포 제약이 있으므로 조사 결론 없이 후반 단계까지 미루지 않습니다. |

### 단계별 완료 시나리오

| 단계 | 반드시 통과할 사용자 작업 |
|---|---|
| A | 신규 SSH 키 확인·변경 키 거부, RDP 인증서 오류, TERM-01~04·TUI, 연결/인증/retry 중 닫기, EOF, 설정 실패 안내를 검증합니다. 로그 노출과 미처리 화면 경로가 없어야 합니다. |
| B | profile 저장 후 재시작 → key/agent/MFA 접속 → SFTP 파일 전송 → gateway 경유·터널 → history 검색·출력 저장 → 단절·정상 종료를 검증합니다. Telnet/Serial/Local과 RDP/VNC 기본 작업도 확인합니다. |
| C | 2/4 pane·창 분리 → focus/IME/clipboard/resize → 원격 파일 편집·충돌 복구를 검증합니다. 다중 세션 성능과 서버 matrix 결과를 기록합니다. |
| D | 새 Windows 환경에서 설치·portable 각각 X11 GUI 실행·종료와 Unix 도구·지속 home·도구 업데이트를 검증합니다. 외부 WSL만 연결한 결과로 portable 완료를 판정하지 않습니다. |
| E | 매크로·동시 입력·추가 프로토콜·RDP 확장·내장 서버/도구/plugin·기업 배포를 비교 항목별로 검증합니다. 44개 묶음과 K29의 세부 목록에 미구현·미검증이 남으면 전체 동등성 완료로 표시하지 않습니다. |

단계는 사용자 결과를 묶은 것이며 번호순 작업과 일대일 대응하지 않습니다. 예를 들어 K15·K16의 기본 검증은 B단계, 다중 pane·고해상도 검증은 C단계입니다. 최종 동등성의 범위와 검증은 사용자 수요가 낮다는 이유로 제거하지 않습니다.

## 확장 설계

전체 GUI·네트워크 스택을 교체하기보다 현재 모듈과 코딩 스타일을 유지하시는 방향을 권장합니다.

| 영역 | 제안 | 완료 기준 |
|---|---|---|
| 저장 profile | 실행 중 Session과 ConnectionProfile을 분리합니다. profile은 ID·폴더·프로토콜 설정·credential 참조를, 실행 상태는 connection state·cancel handle·capability를 보관합니다. | restart 후 폴더·profile 복원, 중복·손상 schema 처리, 비밀값 제외 export가 가능합니다. |
| form·설정 | Welcome draft를 session별로 두고 전역 기본값 → profile override → 실행 중 값 순서로 적용합니다. 숫자·열거 입력을 검증합니다. | 탭 간 입력이 섞이지 않고 invalid 입력을 조용히 기본값으로 바꾸지 않습니다. |
| 연결 정책 | worker를 유지하고 timeout·retry·cancellation·trust를 공통 타입으로 전달합니다. 오류는 문자열 검색 대신 구조화합니다. | 연결 시작 즉시 취소할 수 있고 종료 확인·retry 제한·jitter·인증 실패 중단이 일관됩니다. |
| SSH service | terminal·SFTP·forwarding이 channel을 생성하도록 handle 소유·인증·종료 경계를 설계합니다. gateway는 transport로 재사용합니다. | UI가 내부 library handle에 직접 의존하지 않고 여러 기능이 같은 신뢰 정책을 사용합니다. |
| terminal | 재현 4건을 회귀로 고정하고 cursor bounds·wrap pending·wide cell 불변식을 정리합니다. ConPTY 보정은 backend별로 제한합니다. | 실제 TUI·IME·grapheme를 검증합니다. 직접 보완과 검증된 core 도입을 비교한 뒤 결정합니다. |
| 원격 화면 | clipboard·resize·audio·file transfer capability를 UI에 전달하고 viewport·focus를 명시합니다. | 미지원 옵션이 명확하고 pane별 좌표·IME·modifier release가 맞습니다. |
| 성능·저장 | bounded queue·backpressure·mouse 병합·frame 병합·설정 debounce/atomic replace를 적용합니다. | 부분 frame의 의존 순서를 보존하고 출력·입력 지연과 메모리를 측정합니다. |
| credential | Windows Credential Manager/DPAPI 등 OS 보관을 우선 검토합니다. agent 인증과 forwarding 동의 범위를 구분합니다. | profile JSON·export·진단 로그에 비밀번호·passphrase·토큰이 포함되지 않습니다. |

### RDP·VNC 후속 범위

| 영역 | 작업 순서·주의점 |
|---|---|
| RDP 인증 | TLS chain·hostname·signature 검증, 사설 CA·fingerprint/pinning, domain·자격 증명 UI, 오류 분류와 NLA 정책 검증을 진행합니다. Kerberos/KDC proxy는 별도 기업 환경 범위입니다. |
| RDP 화면 | DisplayControlClient 등록·협상·callback·resize debounce를 연결합니다. GFX 처리 가능한 capability·명령을 협상하고 codec fallback을 검증합니다. |
| RDP 기능 | audio 실제 재생·server cursor·clipboard 이미지/파일을 검증하고 drive·printer·port·smartcard·monitor·gateway를 확장합니다. |
| RDP platform | 현재 Windows clipboard factory를 유지합니다. Linux/macOS가 범위에 들어오면 텍스트 backend를 먼저 추가하고 ownership·polling·loop suppression을 검증합니다. 현재 Windows 대체보다 후순위입니다. |
| VNC frame | JPEG decode·RGBA 변환을 구현하거나 JPEG가 오지 않도록 협상·서버 설정을 제한합니다. Raw·ZRLE·Tight·CopyRect·cursor 조합별 정확도를 검증합니다. |
| VNC 복구·기능 | handshake timeout·retry 취소·오류별 UX, OS clipboard 양방향, SetDesktopSize, coordinate·lock key·IME를 보완합니다. |
| 업스트림 추적 | 과거 DVC V1/V2 협상 문제·EGFX 게시 대기 기록은 해당 의존성 버전에서 재검증합니다. 최신 게시 상태를 추정하거나 upgrade만으로 해결된다고 판단하지 않습니다. |

### 전체 기능 동등성의 남은 범위

| 범위 | 내용·권장 순서 |
|---|---|
| 추가 프로토콜 | K11 독립 SFTP → K25 FTP/FTPS·Mosh·Rlogin/Rsh, K21 X11 이후 K26 XDMCP 순서입니다. legacy 평문 기능은 사용자가 명시적으로 선택하게 합니다. |
| X11 | 기존 Windows X server 관리부터 검토하고 cookie·DISPLAY·포트·clipboard·keyboard·OpenGL·수명주기를 통합합니다. 자체 X server 구현은 별도 대규모 과제입니다. |
| Unix 환경 | WSL/MSYS2 연동·배포판 선택·bash/grep/awk/sed/rsync·지속 home·도구 업데이트입니다. 외부 WSL 연동과 별도 설치 없는 portable 환경은 다른 완료 조건입니다. |
| 자동화·도구 | snippet·macro·동시 입력·키 관리·파일 비교·네트워크 진단·내장 서버·plugin입니다. 다중 실행은 대상·취소·비밀값 제외 정책을 먼저 정합니다. |
| 기업 배포 | K28 공유 profile·정책·사전 설정·설치·업데이트·rollback·권한 제한 환경입니다. Professional의 제한 없는 세션·터널·매크로·daemon 사용과 사용자화도 최종 범위에 포함합니다. |
| 이전 지원 | MobaXterm 세션·폴더 import는 샘플·공개 형식으로 검증하고 미지원 옵션을 보고합니다. 비밀번호 이전을 암묵적으로 지원하지 않습니다. |

이 표는 개발 제안이며 MobaXterm의 내부 구현이나 모든 최신 옵션을 검증했다는 의미는 아닙니다. 업스트림 API·crate 선택은 구현 시 공식 자료와 실제 버전으로 확정하셔야 합니다.

## 의존성 업그레이드 기록

2026-10-06에 crates.io API의 최신 안정·비-yanked 버전과 실제 Cargo resolution을 확인했습니다. Rust 1.94.0에서 빌드 가능한 범위로 Cargo.toml과 Cargo.lock을 갱신했습니다. 직접 crate의 pre-release를 새로 선택하지 않았으나 IronRDP가 요구하는 간접 crypto pre-release는 남아 있습니다.

| 항목 | 적용 내용 |
|---|---|
| RDP | ironrdp 0.14.0 → 0.17.0, 관련 core·input·tokio·tls·dvc·clipboard·audio crate를 함께 갱신했습니다. |
| 비동기·VNC·Windows | tokio 1.50.0 → 1.53.2, tokio-serial 5.4.5 → 5.5.0, vnc-rs 0.5.3 → 0.6.0, windows-sys 0.59.0 → 0.61.2입니다. |
| 기타 | bytes·chrono·env_logger·log·futures·tokio-util·bytemuck·serde·serde_json과 호환되는 간접 의존성을 갱신했습니다. |
| 이미 최신 | iced 0.14.0, nectar 0.4.0, portable-pty 0.9.0, unicode-width 0.2.2, vte 0.15.0은 유지했습니다. |
| 사용하지 않는 선언 | russh-keys와 직접 async-trait를 제거했습니다. async-trait는 필요한 crate의 간접 의존성으로 남아 있습니다. |
| 새 직접 의존성 | ironrdp-egfx 0.3.0은 이동된 GFX PDU 타입을 사용하기 위해 추가했습니다. picky-krb는 아래 호환성 제약을 위해 직접 선언합니다. |

### 최신 버전 적용 예외

**모든 crate를 무조건 최신으로 적용한 상태는 아닙니다.** 다음 두 제약은 실제 Cargo resolution·컴파일 실패로 확인했습니다. Cargo.toml에서 exact version으로 고정하여 이후 cargo update에서도 같은 실패 조합을 다시 선택하지 않게 했습니다.

| 예외 | 적용 버전 | 확인한 이유 |
|---|---|---|
| russh | 0.55.0 유지, 최신 확인값은 0.64.1입니다. | 최신 russh의 curve25519-dalek 5 안정 버전과 IronRDP connector의 picky 7.0.0-rc.25가 고정한 5.0.0-rc.1이 충돌합니다. 중간 버전도 ecdsa·rand_core 제약 또는 pkcs5/RSA API 컴파일 오류가 있어 현재 그래프에서는 기존 버전만 빌드가 확인되었습니다. |
| picky-krb | 0.12.4, 최신 확인값은 0.12.5입니다. | sspi 0.21.3이 0.12.5에 추가된 GssApiMessageError::InvalidMechanismOid를 처리하지 않아 non-exhaustive match 컴파일 오류가 발생했습니다. |

확인한 russh 실패 범위는 0.64/0.63/0.62 계열의 crypto constraint, 0.61/0.60.2~3의 pre-release 고정, 0.60.1의 pkcs5 함수 변경, 0.59~0.57의 rand_core 고정, 0.56.0의 내부 RSA RNG adapter입니다. upstream을 임의로 fork하거나 암호화 source를 수정하지 않았습니다. 향후 connector·sspi의 호환 버전이 게시되면 두 pin을 함께 재평가하셔야 합니다.

최신값의 근거는 [crates.io API](https://crates.io/api/v1/crates/russh), [IronRDP 0.17.0](https://docs.rs/crate/ironrdp/0.17.0), [EGFX 0.3.0](https://docs.rs/crate/ironrdp-egfx/0.3.0)와 다운로드한 해당 버전의 Cargo.toml·source입니다. 검색 페이지의 cached 버전보다 API와 Cargo의 실제 결과를 우선했습니다.

### API 호환 수정과 검증

수정은 src/connection/rdp.rs의 의존성 API 대응에 집중했습니다.

- GFX PDU를 ironrdp-egfx의 GfxPdu·RawCapabilitySet 변환 API로 옮겼습니다. 기존 custom GfxProcessor를 유지하며 새 codec feature는 켜지 않았습니다.
- ActiveStageBuilder에 협상한 channel·share ID를 전달하고 DeactivateAll에서 activation factory로 재활성화합니다. 새 share ID·pointer·framebuffer 상태를 반영합니다.
- 제거된 connector::legacy decode 경로를 pdu::mcs·rdp::headers로 옮기고 새 Config 필드는 기존 동작을 유지하는 기본값으로 설정했습니다.
- mutable clipboard API와 새 file contents/copy 메시지를 backend 채널로 전달합니다. 실제 파일·이미지 clipboard 동작은 미검증입니다.
- GFX capability의 wire version과 frame acknowledgement의 고정 byte fixture 테스트 2개를 추가했습니다.

| 검증 | 결과 |
|---|---|
| cargo check --locked --all-targets | API 호환 수정 후 통과했습니다. |
| cargo test --locked | 기존 6개와 새 GFX 2개, 총 8 passed·0 failed입니다. |
| cargo deny check licenses | 통과했습니다. OpenSSL allowlist가 graph에 없어 license-not-encountered 경고가 있습니다. |
| cargo build --release --locked | Windows x64 릴리즈 빌드가 통과했습니다. |
| cargo deny check advisories | GitHub RustSec DB fetch의 네트워크 오류로 검사하지 못했습니다. 취약점이 없다는 판정이 아닙니다. |
| 실서버·GUI | 이번 업그레이드에서는 실행하지 않았습니다. RDP 재활성화·clipboard·audio, VNC 입력·화면 회귀가 필요합니다. |

기존 FieldFocused·RemoteDisplayInputs dead_code 경고는 남아 있습니다. SSH/RDP trust, terminal 오류·종료 정책 등 기존 P0 문제가 의존성 갱신만으로 해결된 것은 아닙니다. 버전 표·license 구성은 [구조와 구현 현황](architecture.md#의존성과-라이선스)에서 확인하시면 됩니다.

## 검증 계획

### 최초 분석의 기준선

| 항목 | 결과·범위 |
|---|---|
| 2026-10-06 소스 분석 | 기존 `cargo test --locked --offline` 6 passed, 0 failed입니다. 설정 persistence 4건·State 2건입니다. |
| 컴파일 | rustc/cargo 1.94.0에서 test target이 컴파일되었습니다. release·GUI 실행·설치는 이번 분석에서 확인하지 않았습니다. |
| 경고 | FieldFocused·RemoteDisplayInputs 미사용 dead_code 경고가 있습니다. |
| terminal | headless 독립 실행으로 오류 4건을 재현했습니다. [결과](architecture.md#terminal-재현-결과)에 있습니다. |
| 의존성 추적 | TLS verifier 우회, resize의 DisplayControl 전제를 확인했습니다. 실제 서버 handshake·resize 테스트와는 구분합니다. |
| 문서 통합 | 원본 19개 파일을 SHA-256 검증하여 백업했습니다. Windows metadata로 31개 직접 의존성을 확인했습니다. |
| 최초 license 검사 | 문서 통합 시 offline cargo-deny는 미캐시 의존성 때문에 실패했습니다. 이후 업그레이드의 online 검사 통과는 위 기록에 있습니다. |

최초 문서 통합 자체는 테스트·실서버 통과 상태를 추가하지 않았습니다. 이후 의존성 업그레이드 검증은 위 기록에서 별도로 관리합니다. 과거 Windows clipboard·XRDP 기록은 기존 기록으로 유지하고 commit·환경이 명확한 새 결과로 갱신하셔야 합니다.

### 자동 검증

아래는 개발 변경·릴리즈 시 실행할 체크입니다. 이번 문서 정리에서 모두 실행·통과한 목록은 아닙니다.

```powershell
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
cargo deny check licenses
cargo build --release --locked
```

현재 binary 프로젝트에는 lib target이 없으므로 과거 `cargo clippy --lib` 절차는 사용하지 않습니다. 기존 warning으로 strict clippy가 실패하면 원인·정리 여부를 기록하고 무조건 성공했다고 처리하지 않습니다. license scan에는 필요한 crate cache·네트워크가 필요합니다.

CI에는 license 검사·Windows x64 태그 release 빌드와 K01에서 추가한 Windows PR check/test·선별 Clippy가 있습니다. 새 workflow의 GitHub 실행은 미확인이며 release smoke는 backlog입니다. 전체 formatting·strict warning 정리는 기존 실패를 별도 작업으로 해결한 뒤 완료 처리합니다. 새 codec·runtime·외부 바이너리를 추가하면 의존성·license·빌드 조건을 함께 검증하셔야 합니다.

### 실서버·장비 검증

| 영역 | 환경·시나리오 | 통과 조건 |
|---|---|---|
| SSH 신뢰·인증 | OpenSSH, 신규/일치/변경 host key, key/passphrase, agent, MFA, 인증 중 취소 | 신뢰 변경이 차단되고 인증·취소 결과가 명확합니다. |
| terminal | bash·PowerShell, vim·less·top·tmux, Ctrl/Alt/F-key, 한글·결합 문자·emoji, resize·alternate screen | 화면·cursor·복사 결과가 맞고 crash가 없습니다. |
| SFTP | 대용량·다중 파일, 한글 경로, symlink, 권한 오류, 동명 충돌, 단절·취소 | 전송 결과·checksum·복구 상태가 명확하며 파일 손상이 없습니다. |
| gateway·터널 | jump host 뒤 SSH/SFTP/RDP/VNC, local·remote·SOCKS, 포트 충돌·gateway 단절 | 대상 연결·정리·오류 표시와 재시도 정책이 일관됩니다. |
| RDP | Windows RDS·XRDP, NLA, 사설 CA·인증서 변경, clipboard·audio·resize·lock key·단축키 | 서버별 지원 여부를 기록하고 협상한 화면 경로가 정확합니다. |
| VNC | TigerVNC·x11vnc·TightVNC·UltraVNC, Raw/ZRLE/Tight/JPEG·CopyRect·cursor | 모든 협상 경로에서 화면 갱신과 입력이 맞고 matrix에 실측 결과가 있습니다. |
| Telnet·Serial | TTYPE/ECHO/SGA/NAWS, 실제 USB Serial·장비, parity·flow control·break·분리 | 협상·송수신·장치 분리·설정 변경이 예상대로 동작합니다. |
| 종료·장애 | 연결 중 탭 닫기, 연결 100회 반복, 서버 EOF·인증 실패·timeout·재접속 | 작업 종료가 확인되고 worker·socket·자식 프로세스가 누적되지 않습니다. |
| 성능 | 다중 세션, 1080p/고해상도, 대량 로그, LAN·지연·손실 | 입력 반응 p50/p95/p99, CPU·RAM·queue 깊이, 안정 상태를 기록합니다. |
| 배포 | 새 Windows 계정, 설치 경로 쓰기 금지, portable 폴더, 업데이트 실패·rollback | 실행·설정 저장·로그·복구가 동작하고 안내가 명확합니다. |
| profile·credential 이전 | 재시작·schema migration·손상 파일·export/import·MobaXterm 샘플·vault 삭제 | 폴더·연결 옵션이 복원되고 미지원 옵션을 표시하며 비밀값은 export·로그에 없습니다. |
| X11·XDMCP | 새 Windows 계정, xterm·GUI·OpenGL 앱·clipboard·한글 입력, SSH 경유·원격 desktop | DISPLAY/cookie·접속 범위·입력·종료가 맞고 server 프로세스가 남지 않습니다. OpenGL 등 지원 범위를 기록합니다. |
| Unix runtime | 설치/portable·WSL 유무, bash/grep/awk/sed/rsync·지속 home·도구 설치/업데이트 | 선택한 배포 방식에서 별도 설치 요구와 실행 결과가 일치하고 home·설정·업데이트 실패를 복구합니다. |
| 추가 프로토콜 | FTP active/passive·FTPS 인증서·data channel, Mosh bootstrap/UDP 단절, Rlogin/Rsh | 프로토콜별 접속·전송/입력·취소·복구를 검증하고 평문 경로는 명시적으로 선택합니다. |
| 작업 UI·자동화 | 2/4 pane·분리/재결합, keymap·CLI·그룹 실행, 매크로·동시 입력 | focus/IME/resize·입력 대상·중지·실패·비밀 입력 제외가 동작하고 layout/profile이 복원됩니다. |
| 확장·기업 배포 | RDP 리디렉션·monitor, 내장 서버/도구/plugin, 사전 설정·공유 정책 | 기능별 권한·협상·시작/중지·업데이트를 검증하고 K29의 세부 목록과 결과가 대응합니다. |

### 원격 서버 결과 표

현재 다중 벤더 결과는 미검증입니다. 과거 VNC matrix의 TBD를 성공 결과로 옮기지 않았습니다. 다음 표를 새 측정 결과로 갱신하시면 됩니다.

| 대상 | OS·서버 버전 | 인증·협상 | 화면·입력·clipboard·resize | p50/p95/p99 | 복구·결과 |
|---|---|---|---|---|---|
| Windows RDS | 미기록 | NLA·인증서 정책별 검증 필요 | 미검증 | 미측정 | 미검증 |
| XRDP/LXQt | 기존 기록 외 버전 미기록 | TLS·lock key·DVC 재검증 필요 | 미검증 | 미측정 | 미검증 |
| TigerVNC | 미기록 | Password, encoding·PixelFormat 기록 필요 | 미검증 | 미측정 | 미검증 |
| x11vnc | 미기록 | Password, encoding·PixelFormat 기록 필요 | 미검증 | 미측정 | 미검증 |
| TightVNC | 미기록 | Password, Tight/JPEG 검증 필요 | 미검증 | 미측정 | 미검증 |
| UltraVNC | 미기록 | Password, encoding·PixelFormat 기록 필요 | 미검증 | 미측정 | 미검증 |
| RealVNC | 선택 검증 대상 | 보안 타입·호환성 기록 필요 | 미검증 | 미측정 | 미검증 |

공통으로 commit·debug/release·OS·서버 버전·LAN/WAN 조건을 기록합니다. Idle 60초, 빠른 텍스트 스크롤 60초, 화면 변화/영상 120초를 사용하고 ticks/events/full/rect/forced_full_batches/jpeg_events/rect_only_streak, CPU·RAM·queue 깊이·입력 반응을 수집합니다. Raw baseline과 ZRLE/Tight를 비교하여 품질·지연·대역폭으로 우선순위를 정합니다. 서버별 예외는 재현 근거가 있을 때만 profile 정책으로 추가합니다.

같은 PC·서버·네트워크에서 MobaXterm과 비교한 후 성능 기준을 정하시는 것이 좋습니다. 현재 측정값이 없어 FPS·RAM·지연의 우열을 주장하지 않습니다. 1080p RGBA 한 장은 약 7.9MiB라는 계산도 queue 점유율 측정과 구분해야 합니다.

### 결과 기록 형식

이 문서의 결과 표를 갱신하거나 PR·release 증적에 다음 필드를 남기시면 됩니다. 비밀번호·토큰·clipboard 본문은 기록하지 않습니다.

| 필드 | 기록 내용 |
|---|---|
| 기준 | 날짜, commit, 빌드 모드·target, OS·서버·장비 버전입니다. |
| 조건 | 인증 방법·협상 codec·PixelFormat·해상도, 네트워크·관련 설정입니다. |
| 실행 | 입력·화면·resize·clipboard·장애·취소 순서입니다. |
| 결과 | 기대·관측, 성공·실패·미검증 구분, 증적 위치입니다. |
| 성능 | p50/p95/p99, CPU·RAM·queue, 비교 baseline입니다. |
| 후속 | 실패 원인, 제한·회피 방법, 연결된 작업 ID입니다. |

연결 → 화면 정확도 → 입력·취소 → 기존 프로토콜 회귀 → 서버 matrix·배포 순서로 확인하셔야 합니다. 공통 renderer·입력 변경은 RDP/VNC 모두를, terminal 변경은 SSH/Telnet/Serial/Local을 검증해야 합니다.

## 릴리즈 체크리스트

각 체크는 릴리즈별 증적이 있을 때 완료 처리합니다.

### 기능·신뢰·품질

- [ ] P0 작업이 해결되었거나 배포 용도와 제한이 명확합니다.
- [ ] SSH 키·RDP 인증서의 최초 신뢰·변경 차단·사설 CA 정책을 검증했습니다.
- [ ] 민감한 credential·clipboard·토큰이 진단 로그와 export에 없습니다.
- [ ] 연결·인증·retry 중 취소, 서버 EOF, 탭·앱 종료 시 worker·socket·자식 프로세스 정리를 확인했습니다.
- [ ] terminal 회귀, 선택·IME·TUI·resize, RDP/VNC·Telnet·Serial·Local smoke를 확인했습니다.
- [ ] fmt·check·clippy·test·release build 결과와 기존 warning을 기록했습니다.
- [ ] 새 Windows 계정·쓰기 제한 설치 경로·portable 폴더에서 설정·로그·시작을 확인했습니다.
- [ ] 지원 서버·장비·codec·미지원 기능과 실패 사례가 문서에 반영되어 있습니다.
- [ ] 이번 릴리즈의 목표 단계·K 작업 ID·기능 비교 번호에 구현·검증·배포 결과를 연결했습니다. 전체 동등성을 표방하는 릴리즈는 D·E단계와 K29의 세부 목록까지 완료했습니다.

### 의존성·라이선스

- [ ] Cargo.lock과 Windows metadata를 확인하고 [의존성 표](architecture.md#직접-의존성)를 갱신했습니다.
- [ ] 전체 배포 의존성·외부 X server·Unix 도구·plugin·codec의 재배포 조건과 고지를 확인했습니다.
- [ ] `cargo deny check licenses`와 License Check workflow의 결과·경고를 기록했습니다.
- [ ] [LICENSE](license/LICENSE), [MIT](license/LICENSE-MIT), [Apache-2.0](license/LICENSE-APACHE), [MPL-2.0](license/LICENSE-MPL-2.0)의 필요 원문이 배포 구성에 포함됩니다.
- [ ] [OFL](../assets/fonts/OFL-1.1.txt)과 [D2Coding 고지](../assets/fonts/D2Coding-LICENSE-NOTICE.txt)를 확인했습니다.
- [ ] serialport 등 MPL 구성요소의 source 제공 위치와 변경 여부·변경 파일 제공 방안을 안내했습니다.
- [ ] Cargo.toml의 `MIT OR Apache-2.0`과 해당 소스 SPDX 고지가 일치합니다.

### 배포 정보

- [ ] Cargo.toml 버전, tag, release note와 artifact 버전이 일치합니다.
- [ ] README의 실행·기능·제약과 이 문서의 검증 결과를 갱신했습니다.
- [ ] 배포본에 필요한 고지·실행 자료가 포함되고 다운로드한 artifact를 실제로 실행했습니다.
- [ ] 업데이트·rollback이 지원 범위라면 실패·복구 시나리오를 검증했습니다.

현재 release workflow는 태그 push에서 Windows x64 실행 파일을 빌드·업로드합니다. 고지 파일 묶음·installer·서명·자동 업데이트·artifact smoke는 별도 보완 대상입니다. 코드 변경 없이 checklist만 완료 처리하지 않으셔야 합니다.

## 문서 통합과 백업

문서 통합 시 `docs/archived/2026-10-06`에 기존 상대 디렉터리를 유지해 복사하고 19개 파일의 SHA-256 일치를 확인했습니다. 현재 백업에는 설명 문서 14개가 있으며 license 원문은 docs/license에, MPL 안내는 architecture의 license 절에 있습니다. 향후 기록은 새 날짜의 snapshot으로 보관하시면 됩니다.

| 이전 문서 | 현재 위치 |
|---|---|
| [README](archived/2026-10-06/README.md) | 루트 README의 목표·실행·지원 범위입니다. |
| [THIRD_PARTY_LICENSES](archived/2026-10-06/THIRD_PARTY_LICENSES.md), [MPL 안내의 통합 내용](architecture.md#mpl-20과-간접-의존성) | architecture의 의존성·license와 이 문서의 배포 점검입니다. |
| [아키텍처](archived/2026-10-06/docs/kterm_architecture_overview.md) | architecture의 모듈·흐름·설정·protocol 설명입니다. |
| [버그 해결 기록](archived/2026-10-06/docs/kterm_bug_resolution_report.md), [walkthrough](archived/2026-10-06/docs/kterm_walkthrough_final.md) | architecture의 현재 구현·과거 수정·재현 문제입니다. |
| [UI 계획](archived/2026-10-06/docs/implementation_plan.md), [추가 UI 계획](archived/2026-10-06/docs/new_implementation_plan.md) | 확장 설계와 profile·pane·설정·검색 backlog입니다. |
| [RDP 계획](archived/2026-10-06/docs/rdp_integration_plan.md), [NLA 계획](archived/2026-10-06/docs/nla_credssp_implementation_plan.md) | architecture의 RDP 현황, 개발 우선순위·RDP 후속 범위·서버 검증입니다. |
| [VNC 계획](archived/2026-10-06/docs/vnc_integration_plan.md), [호환성 matrix](archived/2026-10-06/docs/vnc_compatibility_matrix.md) | architecture의 VNC 현황, 개발 우선순위·원격 서버 결과 표입니다. |
| [작업 목록](archived/2026-10-06/docs/task.md) | 단계별 목표와 통합 backlog입니다. |
| [기능 격차 분석](archived/2026-10-06/docs/mobaxterm-gap-analysis.md) | architecture의 확인된 문제, 이 문서의 비교표·개발·검증 계획입니다. |
| [릴리즈 체크리스트](archived/2026-10-06/docs/RELEASE_CHECKLIST.md) | 이 문서의 릴리즈 체크리스트입니다. |

라이선스 원문·폰트 고지는 실행 문서 수와 별개의 배포 자료로 유지합니다. 원본 백업에는 역사적인 경로·계획·검증 표현이 그대로 남아 있으며 현재 지원 범위는 통합 문서에서 관리합니다.
