# 개발·검증 계획

Windows에서 MobaXterm을 대체하기 위한 기능 격차, 개발 순서, 완료 조건과 배포 절차를 이 문서에서 관리합니다. 현재 코드·문제의 상세 근거는 [구조와 구현 현황](architecture.md), 사용 방법은 [README](../README.md)에 있습니다.

- 정리일: 2026-10-06, 한국 시간 기준입니다.
- 현재 버전: kterm 0.1.2입니다. 분석 출발점은 commit `6f4ec55d3d084746febf14ae2e0815875107c0b0`이며 이후 의존성·API 변경을 반영했습니다.
- 아래 설계·backlog·완료 조건은 **제안**입니다. 이미 구현한 것으로 해석하지 않으셔야 합니다.
- 구현, 실서버 검증, 배포 완료를 각각 구분합니다. 인력·검증 환경이 확정되지 않아 완료 날짜는 추정하지 않았습니다.

## 목표와 단계

현재는 여러 프로토콜을 연결하는 초기 통합 클라이언트입니다. 일반적인 서버 관리부터 대체하고 Linux GUI·Unix 도구·추가 프로토콜까지 확장하는 중간 목표가 필요합니다. 기능 동등성이 최종 목표이므로 X11과 자체 Unix 실행 환경을 누락 범위로 남겨두어서는 안 됩니다.

| 단계 | 사용자에게 제공할 결과 | 다음 단계로 넘어갈 조건 |
|---|---|---|
| A: 기본 신뢰·안정성 | 안전하게 연결하고 정상적으로 종료할 수 있습니다. | P0 항목과 terminal 재현 오류가 해결되고 회귀 검증이 있습니다. |
| B: 서버 관리 대체 | 저장 세션으로 key/MFA 로그인하고 파일을 옮기며 bastion·터널을 사용합니다. | profile·SSH 인증·SFTP·gateway·검색·출력 저장의 실제 업무 시나리오가 통과합니다. |
| C: 통합 작업 환경 | 분할 terminal과 검증된 RDP/VNC·원격 편집을 사용합니다. | 서버 matrix, clipboard·입력·resize·장애·성능 결과가 있습니다. |
| D: Linux GUI·Unix 환경 | X11 앱과 로컬 Unix 도구를 kterm에서 사용합니다. | X server·forwarding·도구 환경의 설치/portable 동작과 배포 조건을 만족합니다. |
| E: 전체 범위 확대 | 매크로·동시 실행·추가 프로토콜·내장 도구·기업 배포를 제공합니다. | 누락 기능별 지원 범위와 제한을 공개하고 사용 시나리오를 검증합니다. |

현재 A단계의 선행 문제가 남아 있습니다. B단계에는 저장 profile·고급 SSH·파일 전송·gateway 모듈을 새로 추가해야 하며 D·E단계에는 runtime·protocol·배포 과제가 포함됩니다.

## MobaXterm 기능 격차

2026-10-06 분석에서 확인한 [공식 기능 안내](https://mobaxterm.mobatek.net/features.html), [사용 문서](https://mobaxterm.mobatek.net/documentation.html), [에디션 비교](https://mobaxterm.mobatek.net/download.html)를 기준으로 했습니다. MobaXterm을 같은 서버에서 비교 실행하지는 않았습니다.

아래 36개는 공개 기능을 작업 단위로 묶은 분석 목록이며 모든 옵션을 개별 집계한 것은 아닙니다. **기본 구현 3개, 부분 구현 13개, 미구현 20개**입니다. 약 56%에는 대응 기능이 없고 약 36%는 보완이 필요합니다. 서로 다른 규모의 항목을 세었으므로 개발 완료율·품질 점수가 아닙니다.

- 기본 구현: 표에 적힌 좁은 기본 동작의 경로가 있습니다. 제품 전체와 동등하다는 의미는 아닙니다.
- 부분 구현: 일부 코드·UI가 있으나 필수 동작·검증·호환성이 부족합니다.
- 미구현: 현재 제품에 통합된 대응 기능이 없습니다. 외부 CLI 실행 가능 여부와 구분합니다.

| 번호 | 비교 항목 | 판정 | 현재 상태와 근거 |
|---:|---|---|---|
| 01 | 다중 탭 생성·선택·닫기 | 기본 구현 | `Session`, `TabSelected`, `CloseTab`이 있습니다. 순서 변경·복제·창 분리는 없습니다. [model](../src/app/model.rs), [update](../src/app/update.rs) |
| 02 | 기본 terminal 출력·스크롤·색상 | 부분 구현 | CSI·SGR, 최대 10,000줄 history가 있지만 ECH 오류와 경계 panic이 재현되었습니다. [terminal](../src/terminal.rs) |
| 03 | TUI·terminal mode 호환성 | 부분 구현 | 일부 CSI만 처리하며 alternate screen·DEC private mode·application cursor mode 등이 빠져 있습니다. [terminal](../src/terminal.rs) |
| 04 | 한글·IME·문자폭·reflow | 부분 구현 | IME preedit/commit과 문자폭 처리가 있습니다. 한글 reflow 오류가 재현되었고 grapheme 단위 저장은 없습니다. [terminal](../src/terminal.rs), [subscription](../src/app/subscription.rs) |
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
| 30 | RDP NLA·도메인·인증서 정책 | 부분 구현 | CredSSP 옵션은 연결됩니다. domain은 `None`이며 TLS 신뢰 검증을 우회합니다. [rdp](../src/connection/rdp.rs), 5.2절 |
| 31 | RDP clipboard·오디오 | 부분 구현 | Windows clipboard와 rdpsnd 코드가 있습니다. 텍스트 clipboard는 기존 검증 기록이 있고 오디오는 미검증입니다. [windows](../src/platform/windows.rs), [README](../README.md) |
| 32 | RDP 드라이브·프린터·포트·smartcard | 미구현 | 해당 리디렉션 채널과 사용자 설정이 없습니다. |
| 33 | RDP 동적 크기·화면 구성 | 부분 구현 | resize 메시지·`encode_resize` 호출은 있지만 DisplayControlClient 등록이 빠져 있습니다. 다중 모니터 UI도 없습니다. [rdp](../src/connection/rdp.rs), 5.6절 |
| 34 | VNC 화면·입력·clipboard·크기 | 부분 구현 | Raw·ZRLE·Tight 협상, CopyRect·커서·view-only가 있습니다. JPEG를 버리고 OS clipboard·SetDesktopSize가 없습니다. [vnc](../src/connection/vnc.rs) |
| 35 | 재접속·timeout·취소·종료 | 부분 구현 | VNC 재시도와 TCP timeout은 있습니다. 다른 프로토콜 정책과 연결 중 취소·종료는 부족합니다. [update](../src/app/update.rs), [connection](../src/connection/mod.rs) |
| 36 | 내장 서버·관리 도구·plugin | 미구현 | daemon 관리, 파일 비교·네트워크 도구·plugin 실행 환경이 없습니다. |

비밀번호 기반 단순 명령은 제한적으로 평가할 수 있지만 운영 서버 관리는 신뢰 검증·key/MFA·SFTP·profile 부재로 어렵습니다. bastion 뒤 접속과 Linux GUI·자체 Unix CLI 환경은 제품 내부에서 대체할 수 없습니다. RDP/VNC와 Serial은 서버·장비별 평가가 필요합니다.

## 개발 우선순위

P0는 신뢰·충돌·기본 동작 문제, P1은 일상 서버 관리 대체, P2는 생산성과 범위 확장, P3는 사용 대상이 좁은 기능입니다. S/M/L/XL은 상대적인 범위이며 작업일 추정이 아닙니다.

| 순서 | 우선순위 | 작업 | 관련 위치·선행 조건 | 난이도 | 완료 조건 |
|---:|---|---|---|---|---|
| 1 | P0 | SSH known_hosts·RDP TLS 정책 | ssh, rdp, TLS adapter | M~L | 최초 신뢰 확인, 변경 키·인증서 거부, 사설 CA 정책을 자동·수동 검증합니다. |
| 2 | P0 | terminal 오류와 mode 처리 | terminal, subscription | L | 5.3절 4건이 회귀 테스트에서 통과하고 기본 Ctrl·Alt·F-key와 alternate screen이 동작합니다. |
| 3 | P0 | 연결 취소·종료·오류 상태 | model, update, 모든 worker, PTY | M~L | 연결 중·retry 중·인증 중 탭 종료 시 worker와 프로세스가 종료되며 재시도 폭주가 없습니다. |
| 4 | P0 | clipboard 로그 제거·설정 정합성 | vnc, update, settings | S~M | 민감 내용이 로그에 없고 노출된 옵션마다 실제 적용 여부가 검증됩니다. |
| 5 | P1 | 저장 연결 프로필·최근 접속 | 새 persistence 모듈, model, sidebar | M | 재시작 후 profile·폴더가 유지되고 편집·복제·검색·삭제·재접속이 가능합니다. |
| 6 | P1 | SSH 개인키·agent·MFA | 신뢰 정책 완료 후 ssh·인증 UI | M~L | encrypted key, agent, keyboard-interactive challenge, 취소·인증 실패가 처리됩니다. |
| 7 | P1 | SFTP browser·전송 queue | SSH 서비스 분리, profile | L | 목록·탐색·업/다운로드·취소·진행률·충돌·권한·symlink가 검증됩니다. |
| 8 | P1 | SSH gateway·터널 | transport·인증 재사용 | L | gateway를 통해 SSH/SFTP/RDP/VNC가 연결되고 local/remote/SOCKS tunnel을 관리할 수 있습니다. |
| 9 | P1 | 검색·세션 출력 저장·폰트 설정 | terminal·profile 설정 계층 | M | history 검색·출력 저장·글자 크기·색상·인코딩 변경이 동작합니다. |
| 10 | P1 | RDP/VNC 호환성과 누락 경로 | DisplayControl, GFX, JPEG, clipboard | L | 서버별 화면·입력·clipboard·resize·재접속 결과를 기록합니다. |
| 11 | P1 | CI·회귀·배포 검증 | 기존 workflows 확장 | M | PR마다 Windows check/test·선별 lint가 실행되고 release smoke 결과가 남습니다. |
| 12 | P2 | 분할 pane·탭 이동·창 분리 | session ID·viewport 입력 정책 | L | pane별 focus·resize·IME·clipboard가 독립 동작하고 layout을 복원합니다. |
| 13 | P2 | 원격 파일 편집 | SFTP 완료 | M | 외부 편집기 저장을 감지하고 원격 변경 충돌·upload 실패에서 원본을 보존합니다. |
| 14 | P2 | X11 서버 관리·SSH forwarding | 신뢰·SSH channel 재사용 | XL | Windows X server와 cookie·DISPLAY·포트·수명주기를 관리하고 실제 X11 앱이 표시됩니다. |
| 15 | P2 | Unix 환경·WSL/MSYS2 연동·배포 | local profile·배포 정책 | L~XL | 선택 환경·도구가 설치/portable 시나리오에서 일관되게 실행됩니다. |
| 16 | P2 | snippet·매크로·동시 입력 | 저장 프로필·pane 안정화 | M~L | 대상 목록이 명확하고 실행 취소·오류·재생·비밀값 제외 정책이 동작합니다. |
| 17 | P2/P3 | 추가 프로토콜·RDP 리디렉션·도구 | 대상 수요와 호환성 조사 | L~XL | 7절에서 선택한 범위별 서버 테스트와 배포 조건을 만족합니다. |

우선 수정의 근거는 [확인된 문제](architecture.md#확인된-문제)의 SEC·TERM·LIFE·RDP·VNC·CFG·PERF·INPUT 항목입니다. 기존 UI 개선 계획의 분할·테마보다 신뢰 검증과 공통 terminal·수명주기 문제를 먼저 해결하시는 것이 좋습니다. 장식적인 헤더·효과는 최종 기능 동등성의 선행 조건으로 두지 않습니다.

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
| 추가 프로토콜 | 독립 SFTP → 수요에 따른 FTP/FTPS → Mosh → Rlogin·XDMCP 등입니다. legacy 평문 기능은 사용자가 명시적으로 선택하게 합니다. |
| X11 | 기존 Windows X server 관리부터 검토하고 cookie·DISPLAY·포트·clipboard·keyboard·OpenGL·수명주기를 통합합니다. 자체 X server 구현은 별도 대규모 과제입니다. |
| Unix 환경 | WSL/MSYS2 연동·배포판 선택·bash/grep/awk/sed/rsync·지속 home·도구 업데이트입니다. 외부 WSL 연동과 별도 설치 없는 portable 환경은 다른 완료 조건입니다. |
| 자동화·도구 | snippet·macro·동시 입력·키 관리·파일 비교·네트워크 진단·내장 서버·plugin입니다. 다중 실행은 대상·취소·비밀값 제외 정책을 먼저 정합니다. |
| 기업 배포 | 공유 profile·정책·사전 설정·설치·업데이트·rollback·권한 제한 환경입니다. Professional의 제한 해제·기업 기능은 공통 기능과 별도로 관리합니다. |
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

CI의 현재 구성은 license 검사와 Windows x64 태그 release 빌드입니다. PR check/test·선별 lint·release smoke를 추가하는 것은 backlog입니다. 새 codec·runtime·외부 바이너리를 추가하면 의존성·license·빌드 조건을 함께 검증하셔야 합니다.

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
