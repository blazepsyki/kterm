# kterm의 MobaXterm 대체 가능성 및 기능 격차 분석

- 분석일: 2026-10-06, 한국 시간 기준입니다.
- 분석 대상: kterm 0.1.1, commit `6f4ec55d3d084746febf14ae2e0815875107c0b0`입니다.
- 목표: Windows에서 MobaXterm을 대체하고, 장기적으로 동등한 작업을 수행하는 것입니다.
- 범위: 현재 소스, Cargo 의존성, 기존 문서, CI 설정, 기존 테스트 및 터미널 코어의 독립 재현 결과입니다.
- 이번 작업에서는 제품 소스를 변경하지 않았습니다.

## 1. 결론

**현재 kterm은 여러 프로토콜을 연결하는 초기 통합 클라이언트이며, MobaXterm을 전반적으로 대체할 단계는 아닙니다.** SSH·Telnet·Serial·로컬 셸·RDP·VNC의 기본 경로와 공용 원격 화면 렌더러는 있습니다. 그러나 저장 세션, 파일 전송, SSH 고급 인증·게이트웨이·터널, X11, 작업 자동화가 빠져 있습니다. 터미널 코어에는 화면 손상과 panic을 일으키는 문제가 재현되었습니다.

이 문서에서 정의한 비교 항목 36개 중 기본 범위가 구현된 항목은 3개, 부분 구현은 13개, 미구현은 20개입니다. **약 56%의 항목에는 대응 기능이 없고, 약 36%는 보완이 필요합니다.** 이는 항목 수의 비율이며 개발 완료율이나 실사용 품질 점수는 아닙니다. 탭 생성과 X11 서버 구축을 같은 규모의 일로 취급할 수 없으므로, 단일 숫자로 전체 완성도를 표현하는 것은 부정확합니다.

권장 진행 순서는 다음과 같습니다.

1. SSH/RDP 신뢰 검증, 터미널 오류, 종료·취소 처리, 적용되지 않는 설정을 먼저 해결하셔야 합니다.
2. 저장 세션, SSH 키·agent·MFA, SFTP, SSH gateway와 터널을 추가하시는 것이 좋습니다.
3. 검색·출력 저장·분할 화면·설정 기능을 갖추고 RDP/VNC 호환성을 검증하셔야 합니다.
4. X11 서버와 forwarding, Unix 도구 배포를 별도 과제로 진행하셔야 합니다.
5. 매크로·동시 실행·추가 프로토콜·내장 도구·기업 배포 기능까지 확장하셔야 전체 대체 목표에 가까워집니다.

**판단:** 일반적인 서버 관리 작업을 먼저 대체하는 중간 목표가 필요합니다. 다만 최종 목표가 기능 동등성이라면 X11과 Unix 실행 환경을 범위에서 제외해서는 안 됩니다.

## 2. 조사 방법과 판단 기준

### 2.1 확인 수준

| 구분 | 의미 |
|---|---|
| 확인 | 현재 소스, 의존성 소스, 테스트 출력으로 확인했습니다. |
| 기존 기록 | README와 작업 문서에 기록된 내용입니다. 이번 조사에서 실서버로 재검증하지 않았습니다. |
| 추론 | 코드 구조에서 예상되는 영향입니다. 실사용 재현이나 측정 여부를 별도로 표시했습니다. |
| 제안 | 향후 구현 방향과 완료 조건입니다. 현재 기능을 의미하지 않습니다. |

소스에 코드가 있다는 사실과 서버에서 정상 동작한다는 사실을 구분했습니다. 미구현 판정은 현재 제품에 연결된 모듈, 상태 모델, 메시지, UI, 의존성 및 호출 경로를 기준으로 했습니다. 로컬 셸에서 외부 명령을 실행할 수 있는 것은 해당 기능이 제품에 통합되어 있다는 뜻이 아닙니다.

### 2.2 MobaXterm 비교 기준

2026-10-06에 확인한 공식 공개 기능을 기준으로 했습니다. 현재 판매되는 제품의 공통 기능을 비교하고, Professional의 제한 해제와 배포 기능은 별도로 다뤘습니다. MobaXterm 자체를 설치하여 같은 서버에서 비교 실행하지는 않았습니다.

- 탭, 세션, SFTP, X11, 동시 실행, SSH gateway·터널, 편집기·매크로·비밀번호 관리: [공식 기능 안내](https://mobaxterm.mobatek.net/features.html).
- 분할·분리 창, 파일 전송·추가 프로토콜, RDP 리디렉션, 터미널 검색·출력 저장·설정: [공식 사용 문서](https://mobaxterm.mobatek.net/documentation.html).
- Home/Professional 차이: [공식 에디션 비교](https://mobaxterm.mobatek.net/download.html). Professional은 Home 기능을 포함하고 저장 세션·터널·매크로 등의 제한을 해제하며 기업 배포·보안 설정을 추가합니다. Home의 저장 세션 제한을 동시에 열 수 있는 탭 수로 해석하지 않았습니다.

아래 36개 항목은 공개 기능을 작업 단위로 묶은 분석용 목록입니다. 모든 제품 옵션을 하나씩 세는 목록은 아닙니다. Rlogin·Mosh·FTP/FTPS 등 추가 프로토콜과 Professional 전용 기능은 7절에 추가로 정리했습니다.

## 3. 현재 구조와 구현 자산

### 3.1 구조

| 영역 | 현재 구조와 근거 | 평가 |
|---|---|---|
| UI·상태 | `iced` 0.14, `State` → `Message` → `update` → `view`입니다. [app](../src/app/mod.rs), [view](../src/ui/view.rs)에서 확인했습니다. | 기존 흐름을 유지하면서 기능을 추가할 수 있습니다. |
| 세션 | [model.rs](../src/app/model.rs)의 `Session`에 ID, 화면 종류, terminal, remote display, sender가 있습니다. | 실행 중인 세션 모델은 있지만 저장 연결 프로필과 수명주기 모델은 없습니다. |
| 연결 | [connection](../src/connection/mod.rs)에 프로토콜별 모듈과 공통 입력·이벤트가 있습니다. | 기본 분리가 되어 있지만 인증·취소·재시도 정책은 프로토콜마다 다릅니다. |
| 터미널 | [terminal.rs](../src/terminal.rs)에 `vte` 파서, 셀·그리드·history, Canvas와 IME 위젯이 있습니다. | `vte`는 escape sequence 파서입니다. terminal semantics는 이 프로젝트가 직접 구현합니다. |
| 원격 화면 | [remote_display](../src/remote_display/mod.rs), [renderer](../src/remote_display/renderer.rs)에 RGBA 상태, 부분 갱신, GPU 업로드, 화면 source ID가 있습니다. | RDP/VNC의 재사용 가능한 기반입니다. 서버별 정확도·성능 검증은 별개입니다. |
| Windows 통합 | [windows.rs](../src/platform/windows.rs)에 PTY 실행 및 네이티브 RDP clipboard backend가 있습니다. | 현재 목표인 Windows 대체에 맞습니다. 플랫폼 확장은 후순위로 둘 수 있습니다. |
| 설정 | [settings_persistence.rs](../src/app/settings_persistence.rs)에 JSON 저장·로드가 있습니다. | 일부 값은 저장만 되고 실행에는 반영되지 않습니다. |
| 배포 | [.github/workflows/release.yml](../.github/workflows/release.yml)에 Windows x64 태그 빌드·Release 업로드가 있습니다. | 배포 경로의 기초는 있으나 설치·업데이트·품질 검증은 부족합니다. |

### 3.2 보존할 구현

- 세션 ID로 비동기 이벤트를 라우팅하는 구조를 유지하시는 것이 좋습니다. 배열 index는 탭 삭제로 바뀔 수 있으므로 향후 분할 화면과 창 분리에서도 ID를 기준으로 관리하셔야 합니다.
- `ConnectionInput`·`ConnectionEvent`와 `RemoteInput`의 공통화는 RDP/VNC 확장에 유용합니다.
- 원격 화면의 `source_id`와 `frame_seq`, dirty rectangle 업로드는 탭 전환과 부분 갱신에 이미 대응하고 있습니다.
- Serial의 data bits·stop bits·parity·hardware flow control, SSH keepalive·PTY 크기 갱신, VNC view-only·커서·CopyRect 설정은 실행 경로까지 연결되어 있습니다.
- 설정의 기본값 및 JSON roundtrip 테스트는 확장 시 유지할 가치가 있습니다.

**제안:** 전체 GUI나 네트워크 스택을 교체하기보다 현재 모듈 구조를 유지하고, 저장 프로필·인증·수명주기·파일 전송을 보강하시는 것이 좋습니다. 터미널 코어만은 직접 보완과 검증된 코어 도입을 비교할 필요가 있습니다.

## 4. 기능 격차 표

판정 기준은 다음과 같습니다.

- **기본 구현:** 표에 적힌 좁은 기본 동작의 경로가 있습니다. MobaXterm의 모든 옵션과 품질이 동등하다는 의미는 아닙니다.
- **부분 구현:** 기본 경로나 일부 UI가 있으나 필수 동작·검증·호환성이 부족합니다.
- **미구현:** 현재 제품에 대응하는 통합 기능이 없습니다.

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
| 28 | Serial | 부분 구현 | 기본 송수신과 포트 설정은 있습니다. 장치 선택·break·신호 제어가 없고 실물 테스트는 기존 체크리스트에서 미완입니다. [serial](../src/connection/serial.rs), [task](task.md) |
| 29 | RDP 화면·키보드·마우스 | 부분 구현 | IronRDP와 bitmap·RemoteFX·공용 renderer가 있습니다. GFX의 일부 codec·명령은 무시합니다. [rdp](../src/connection/rdp.rs) |
| 30 | RDP NLA·도메인·인증서 정책 | 부분 구현 | CredSSP 옵션은 연결됩니다. domain은 `None`이며 TLS 신뢰 검증을 우회합니다. [rdp](../src/connection/rdp.rs), 5.2절 |
| 31 | RDP clipboard·오디오 | 부분 구현 | Windows clipboard와 rdpsnd 코드가 있습니다. 텍스트 clipboard는 기존 검증 기록이 있고 오디오는 미검증입니다. [windows](../src/platform/windows.rs), [README](../README.md) |
| 32 | RDP 드라이브·프린터·포트·smartcard | 미구현 | 해당 리디렉션 채널과 사용자 설정이 없습니다. |
| 33 | RDP 동적 크기·화면 구성 | 부분 구현 | resize 메시지·`encode_resize` 호출은 있지만 DisplayControlClient 등록이 빠져 있습니다. 다중 모니터 UI도 없습니다. [rdp](../src/connection/rdp.rs), 5.6절 |
| 34 | VNC 화면·입력·clipboard·크기 | 부분 구현 | Raw·ZRLE·Tight 협상, CopyRect·커서·view-only가 있습니다. JPEG를 버리고 OS clipboard·SetDesktopSize가 없습니다. [vnc](../src/connection/vnc.rs) |
| 35 | 재접속·timeout·취소·종료 | 부분 구현 | VNC 재시도와 TCP timeout은 있습니다. 다른 프로토콜 정책과 연결 중 취소·종료는 부족합니다. [update](../src/app/update.rs), [connection](../src/connection/mod.rs) |
| 36 | 내장 서버·관리 도구·plugin | 미구현 | daemon 관리, 파일 비교·네트워크 도구·plugin 실행 환경이 없습니다. |

집계는 기본 구현 3개, 부분 구현 13개, 미구현 20개입니다. 앞의 MobaXterm 공식 기능·사용 문서를 비교 기준으로 사용했고, 현재 상태는 각 소스 경로에서 확인했습니다.

### 4.1 작업별 대체 가능성

| 작업 | 현재 판단 | 주요 차단 요인 |
|---|---|---|
| 시험 서버에 비밀번호로 접속하여 단순 명령 실행 | 제한적인 평가 사용이 가능합니다. | SSH 키 검증과 terminal 안정성입니다. |
| 운영 서버를 매일 관리 | 아직 대체하기 어렵습니다. | 키/MFA·known_hosts·저장 세션·SFTP·검색·로그가 없습니다. |
| bastion 뒤 서버·DB·원격 데스크톱 접속 | 제품 내부에서는 대체하기 어렵습니다. | gateway와 터널이 없습니다. 외부 도구로 우회하는 것은 통합 대체가 아닙니다. |
| 원격 Linux GUI 프로그램 사용 | 대체할 수 없습니다. | X server와 forwarding이 없습니다. |
| 별도 설치 없는 Unix CLI 환경 | 대체할 수 없습니다. | 자체 도구 환경이 없습니다. |
| Windows RDP 중심 작업 | 기능별 실서버 검증 후 판단하셔야 합니다. | TLS 신뢰, GFX 호환성, 리디렉션·종료·해상도 변경이 부족합니다. |
| VNC 중심 작업 | 서버별 제한적인 평가가 필요합니다. | JPEG 무시, clipboard 부재, 미작성 호환성 결과입니다. |
| Serial 장비 관리 | 실물 장비 검증이 필요합니다. | terminal 오류, 장치 탐색·제어 기능, 장비별 협상·입력 편차입니다. |

## 5. 기능 추가 전에 해결할 문제

### 5.1 SSH 서버 키를 무조건 허용합니다 — P0

**확인:** `src/connection/ssh.rs:18`의 `check_server_key`는 공개키를 사용하지 않고 `Ok(true)`를 반환합니다. known_hosts 조회·fingerprint 표시·키 변경 차단이 없습니다.

**추론:** 서버 신원 확인이 되지 않으므로 운영 서버용 접속 도구의 기본 신뢰 조건을 충족하지 못합니다. 실제 공격 재현은 하지 않았습니다.

**제안:** host·port별 known_hosts, 최초 fingerprint 확인, 변경 키 기본 차단, 승인 기록, hash host와 저장 오류 처리를 구현하셔야 합니다. 비밀번호·개인키 인증과 SFTP·터널에도 같은 신뢰 정책을 적용하셔야 합니다.

### 5.2 RDP TLS도 인증서 검증을 우회합니다 — P0

**확인:** `src/connection/rdp.rs:1245`는 `ironrdp_tls::upgrade`를 사용합니다. Cargo.lock의 `ironrdp-tls`는 0.2.0이며 Cargo.toml은 `rustls` feature를 선택합니다. 로컬 Cargo registry의 `ironrdp-tls-0.2.0/src/rustls.rs:15`에는 `.dangerous().with_custom_certificate_verifier(...NoCertificateVerification)`이 있습니다. `verify_server_cert`뿐 아니라 TLS 1.2/1.3 signature 검사도 성공 assertion을 반환합니다.

프로젝트의 “수동 NoCertificateVerification을 교체했다”는 주석은 인증서 검증이 활성화되었다는 근거가 아닙니다. NLA가 켜져 있다는 사실과 TLS 인증서 신뢰는 별도입니다.

**제안:** TLS adapter에서 정상 chain·hostname·서명 검증을 적용하고 사설 CA와 self-signed 인증서는 fingerprint 확인·명시적 pinning 정책을 제공하셔야 합니다. 인증서 변경을 조용히 수락해서는 안 됩니다. 의존성 업그레이드만으로 해결되었다고 판단하지 말고 실제 verifier를 확인하셔야 합니다.

### 5.3 터미널 코어 오류 4건이 독립 실행에서 재현되었습니다 — P0

프로젝트 소스를 수정하지 않고 임시 Rust 실행 파일에서 현재 `src/terminal.rs`를 직접 포함했습니다. 기존 빌드의 iced·vte·unicode-width 라이브러리를 사용했으며 GUI·네트워크는 실행하지 않았습니다. 아래 출력에서 `_`는 저장된 NUL continuation cell을 표시합니다. 공백은 실제 빈 셀입니다.

| 재현 | 입력·조건 | 기대 | 관측 |
|---|---|---|---|
| ECH가 글자를 삭제·당깁니다. | 1행 8열, `ABCDE\r\x1b[2C\x1b[1X` | `AB DE   ` | `ABDE    ` |
| 행 끝 지우기로 panic이 발생합니다. | 1행 4열, `ABCD\x1b[K` | 정상 종료 | `terminal.rs:237`에서 index 4 / length 4 panic이 발생했습니다. |
| alternate screen이 복원되지 않습니다. | 1행 8열, `BASE\x1b[?1049h\rALT\x1b[?1049l` | `BASE    ` | `ALTE    ` |
| 한글 reflow가 셀을 밀어냅니다. | 1행 6열에 `가A` 출력 후 8열로 resize | `가_A     ` | 변경 전 `가_A   `, 변경 후 `가 _A    ` |

**확인:** `terminal.rs:572` 부근의 ECH(`CSI X`)는 일반 경로에서 뒤 셀을 앞으로 당깁니다. ECH는 해당 위치의 문자를 지우는 동작이며 DCH와 다릅니다. `print`는 행을 채우면 `cursor_x == cols`가 될 수 있는데 `clear_line`은 범위 검사 없이 해당 index를 읽습니다. `csi_dispatch`는 private prefix를 포함한 intermediates를 무시하고 `h/l` mode 처리도 없습니다. reflow는 wide character의 폭과 저장된 continuation cell을 중복 계산합니다.

동작 기준은 [XTerm Control Sequences의 ECH·DECSET·alternate screen 설명](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html)을 참고했습니다.

**추론:** 실제 GUI에서 같은 terminal 출력 경로로 panic이 전달되면 앱이 종료될 수 있습니다. 모든 SSH·Telnet·Serial·로컬 셸이 같은 코어를 사용하므로 영향 범위가 넓습니다. GUI 종료 자체는 이번 조사에서 재현하지 않았습니다.

**제안:** 위 입력을 먼저 회귀 테스트로 고정하고 cursor bounds·wrap pending·wide cell 불변식을 정리하셔야 합니다. ConPTY 잔상 보정은 전체 프로토콜에 적용하지 말고 검증된 backend 조건에서만 적용하셔야 합니다. 현재 보정은 CUP/ECH 동작을 바꾸며 원격 TUI에도 적용됩니다.

추가 확인 항목은 cursor save/restore, alternate buffer, DECCKM·keypad, autowrap·origin mode, 마우스 보고, bracketed paste, OSC title·hyperlink, DEC character set, 결합 문자·emoji입니다. terminal 키 입력은 현재 Ctrl+C/V 외 일반 Ctrl 조합·Alt prefix·F-key에 대한 별도 인코딩이 없어 보완이 필요합니다. `TERM=xterm-256color`와 실제 지원 기능의 차이도 검증하셔야 합니다.

### 5.4 설정이 저장되어도 실제 동작하지 않습니다 — P0/P1

**확인:** 다음 필드는 UI·State·JSON에는 있지만 실행 경로에서 설정값을 사용하지 않습니다.

| 설정 | 실제 상태 | 조치 |
|---|---|---|
| CommonTimeout | SSH/Telnet/RDP의 connect·인증에 적용되지 않습니다. | 공통 단계별 deadline과 취소를 추가하셔야 합니다. |
| UseAgentForwarding | SSH 연결 인자로 전달되지 않습니다. | 기능 구현 전에는 사용 가능 설정으로 노출하지 않으셔야 합니다. |
| Telnet LineEnding·EchoLocally | Enter는 공통 입력 코드에서 CR이며 Telnet worker는 두 값을 받지 않습니다. | 협상 결과와 사용자 override를 구분해 적용하셔야 합니다. |
| Local DefaultShell·StartupArgs·LoginMode | 자동 탐지한 `LocalShellOption`만 사용합니다. | 사용자 설정을 실행 spec에 반영하셔야 합니다. |
| CompactTabStyle | tab height와 spacing은 고정값입니다. | 실제 layout에 반영하셔야 합니다. |
| AutoReconnect | VNC에서만 읽습니다. | 적용 프로토콜을 표시하고 공통 정책을 마련하셔야 합니다. |

근거는 `src/app/update.rs`의 `Connect*` 분기, `src/app/subscription.rs`의 terminal 입력, 각 worker의 함수 인자입니다. SSH keepalive·terminal type, Serial 설정, RDP 옵션, VNC 옵션까지 모두 미적용인 것은 아닙니다.

### 5.5 연결 수명주기·취소·종료가 부족합니다 — P0/P1

**확인:** `CloseTab`은 `Shutdown`을 보내지만 RDP의 `handle_rdp_input`은 `Shutdown`에서 `Ok(())`만 반환합니다(`rdp.rs:652`). 호출하는 worker 루프에는 이 메시지로 종료하는 분기가 없습니다. VNC는 연결 후 루프에서 Shutdown을 보고 종료하지만 retry 대기 중에는 `sleep`하고, handshake를 전체 timeout으로 감싸지 않습니다. UI는 연결 완료 전에는 sender가 없어 worker에 취소를 전달할 수 없습니다.

Telnet·Serial의 unfold 상태는 오류 후 `None`으로 되돌아가 다음 poll에서 다시 연결을 시도할 수 있습니다. 명시적인 재시도 정책·backoff·종료 상태와 다릅니다. SSH EOF callback은 상태 전이를 하지 않으며 channel close는 terminal 문자열만 보냅니다. 로컬 PTY는 자식 프로세스 kill/wait와 EOF 이벤트를 명시적으로 관리하지 않습니다.

**추론:** 연결 중 탭 삭제, 인증 대기, 반복 오류, 정상 서버 종료에서 worker가 지연 종료하거나 UI와 상태가 어긋날 가능성이 있습니다. 실제 프로세스 누수와 재시도 빈도는 측정하지 않았습니다.

**제안:** `Connecting / Authenticating / Connected / Reconnecting / Disconnected / Failed / Closing` 상태를 명시하고, 연결 시작 즉시 cancellation handle을 보관하셔야 합니다. worker의 종료 확인과 자식 프로세스 wait, 재시도 제한·jitter·인증 오류 중단을 구현하셔야 합니다.

### 5.6 RDP 해상도·GFX 지원이 겉보기보다 제한됩니다 — P1

**확인:** `rdp.rs:624`에 `active_stage.encode_resize` 호출은 있습니다. 그러나 `rdp.rs:1230`의 Drdynvc에는 `GfxProcessor`만 등록합니다. Cargo.lock의 `ironrdp-session` 0.8.0 소스 `active_stage.rs:219`를 확인하면 `encode_resize`는 등록된 `DisplayControlClient`가 있어야 데이터를 만들며, 없으면 `None`을 반환합니다. 따라서 현재 wiring으로는 resize 호출만으로 원격 해상도를 바꿀 수 없습니다.

또한 GFX는 V8~V10.7 capability를 광고하지만 `WireToSurface1`의 Uncompressed 외 codec과 `WireToSurface2`를 처리하지 않으며 여러 명령은 기본 분기에서 무시합니다. 기존 bitmap/RemoteFX 경로가 있다는 사실을 GFX 전 기능 지원으로 해석할 수 없습니다.

**제안:** DisplayControl 채널과 capability callback을 연결하고, 처리 가능한 GFX capability·명령만 협상하셔야 합니다. 미지원 codec을 받으면 명확히 fallback하거나 실패를 보고하셔야 합니다. Windows RDS·XRDP의 실제 결과를 구분해 기록하셔야 합니다.

### 5.7 VNC는 Tight 협상과 JPEG 처리 사이에 공백이 있습니다 — P1

**확인:** `vnc.rs:330`에서 ZRLE·Tight·Raw를 협상하지만 `vnc.rs:804`의 `JpegImage`는 로그만 남기고 버립니다. 서버가 보내는 resolution 변경은 수신하지만 사용자 resize는 FullRefresh만 요청합니다. OS clipboard 연동과 SetDesktopSize 송신은 없습니다.

**추론:** Tight 서버가 JPEG rectangle을 사용하면 해당 영역이 갱신되지 않을 수 있습니다. 모든 Tight 연결이 실패한다는 뜻은 아닙니다. 서버별 실제 영향은 미측정입니다.

**제안:** JPEG decode·RGBA 변환을 구현하거나 JPEG를 받지 않도록 협상·서버 설정을 제한하셔야 합니다. 채택한 제한을 사용자에게 표시하고 [VNC 호환성 표](vnc_compatibility_matrix.md)에 실제 결과를 입력하셔야 합니다. 현재 표는 TBD이며 검증 결과가 아닙니다.

### 5.8 민감한 clipboard 내용이 진단 로그로 전달됩니다 — P0

**확인:** `vnc.rs:754`는 수신 clipboard text 전체를 `ConnectionEvent::Data` 문자열에 넣습니다. `update.rs`는 RemoteDisplay의 Data를 `log::info!`로 출력하고 `main.rs`의 logger는 파일에 기록합니다. OS clipboard 기능은 없지만 내용은 로그에 남을 수 있습니다.

**제안:** 내용 대신 방향·길이·성공 여부만 기록하셔야 합니다. 앱 로그와 terminal 기록을 구분하고, 후자는 사용자가 세션별로 켤 수 있게 하셔야 합니다. credential·토큰·clipboard는 진단 로그에서 기본 제외하셔야 합니다.

### 5.9 성능·입력·저장 경로도 보완이 필요합니다 — P1/P2

**확인:** 다수의 입력·출력·frame 채널이 unbounded이며 프레임을 clone하는 경로가 있습니다. `terminal.rs:801`은 CSI마다 debug 파일을 열고 씁니다. 설정 text 변경은 UI update에서 동기 파일 저장을 호출합니다. 원격 mouse 좌표는 sidebar 181px·header 66px를 가정하며 keyboard와 CursorMoved 이벤트는 viewport 여부로 제한하지 않습니다.

**추론:** 대량 출력·고해상도 화면·빠른 입력 시 queue 메모리 증가와 지연, 저장 경로가 느릴 때 UI 지연이 발생할 수 있습니다. 분할 화면이나 sidebar 변경 시 좌표 오류가 생길 수 있습니다. 현재 RAM·CPU·입력 지연은 측정하지 않았습니다. 1920×1080 RGBA 한 장은 약 7.9MiB이므로 full frame 100개가 쌓이면 픽셀 데이터만 약 791MiB입니다. 이는 계산 예시이며 현재 점유율 측정값이 아닙니다.

**제안:** bounded queue와 backpressure, mouse move 병합, frame 병합, viewport·focus 기반 입력 라우팅을 도입하셔야 합니다. 부분 갱신 frame은 이전 갱신을 전제로 하므로 단순 삭제해서는 안 됩니다. terminal debug I/O는 opt-in으로 전환하고 설정 저장에는 debounce·atomic replace·오류 표시를 추가하시는 것이 좋습니다. 실행 파일 옆 portable 경로와 사용자 설정 경로를 명시적으로 구분하셔야 합니다.

## 6. 추가 우선순위와 완료 조건

P0는 운영 사용을 막는 신뢰·충돌·기본 동작 문제, P1은 일상 서버 관리 대체에 필요한 기능, P2는 생산성과 제품 범위 확장입니다. P3는 상대적으로 사용 대상이 좁은 추가 기능입니다. 난이도 S/M/L/XL은 상대적인 범위 판단이며 작업일·일정 추정이 아닙니다.

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

SSH 고급 인증, profile, SFTP는 연관되어 있으므로 구현 전에 공통 SSH 서비스 범위를 정하시는 것이 좋습니다. 반면 X11·Unix runtime은 필요한 규모가 커서 기본 운영 기능 개발과 별도로 진행 상황을 관리하셔야 합니다.

## 7. 전체 기능 동등성을 위한 추가 범위

일상적인 SSH 관리 대체를 달성해도 아래 차이는 남습니다. [MobaXterm 사용 문서](https://mobaxterm.mobatek.net/documentation.html)와 [에디션 비교](https://mobaxterm.mobatek.net/download.html)에 공개된 범위를 기준으로 정리했습니다.

| 범위 | 추가할 내용 | 권장 순서·주의점 |
|---|---|---|
| 추가 프로토콜 | 독립 SFTP, FTP/FTPS, Mosh, Rlogin, XDMCP 등입니다. | SFTP → 필요 시 FTP/FTPS → Mosh → legacy 프로토콜 순으로 권장합니다. 레거시 평문 기능은 사용자가 선택하는 범위로 두셔야 합니다. |
| X11 | multiwindow·windowed 실행, clipboard·keyboard·display 설정, OpenGL 등입니다. | 기존 X server를 관리하는 방식부터 검토하셔야 합니다. 자체 X server 구현은 매우 큰 별도 프로젝트입니다. |
| Unix 환경 | bash·grep·awk·sed·rsync 등 도구, 지속 home, 도구 추가·업데이트입니다. | WSL 연동은 유용하지만 별도 환경 없이 실행되는 portable UX와 동등하지는 않습니다. 번들 여부를 정하셔야 합니다. |
| RDP 업무 기능 | drive·printer·port·smartcard, 관리 접속, domain 정책, monitor·gateway 등입니다. | 리디렉션과 gateway의 실제 필요성을 확인하되 최종 동등성 backlog에는 남기셔야 합니다. clipboard 파일 전송은 텍스트와 별도로 검증하셔야 합니다. |
| 편의 도구 | 키 생성·관리, 파일 비교, 네트워크 진단, 내장 서버, plugin입니다. | 초기에는 외부 도구 연동이 가능합니다. 자체 통합·배포 여부는 별도 완료 조건으로 관리하셔야 합니다. |
| 보안·기업 배포 | credential vault, import/export·팀 profile, 정책, 사전 설정 배포, 업데이트·설치입니다. | Windows Credential Manager/DPAPI 등 OS 보관을 우선 검토하시고, 공유 export에는 비밀값을 기본 제외하셔야 합니다. |
| 기존 환경 이전 | MobaXterm의 세션 목록·폴더를 kterm profile로 옮기는 기능입니다. | 공개·샘플 설정으로 호환 형식을 확인하고 지원하지 않는 옵션을 보고하셔야 합니다. 비밀번호 이전을 암묵적으로 지원해서는 안 됩니다. |

기업용 설정·업데이트를 제품 범위로 정하는 것은 이 분석의 제안입니다. MobaXterm의 특정 내부 동작이나 모든 최신 옵션을 검증했다는 의미는 아닙니다.

## 8. 기존 설계를 유지하는 확장 방향

### 8.1 저장 프로필과 실행 세션을 구분하셔야 합니다

**제안:** 현재 `Session`을 실행 상태로 유지하고 `ConnectionProfile`을 별도로 두시는 것이 좋습니다. profile에는 ID·표시명·폴더·protocol별 설정·credential 참조를, 실행 상태에는 connection state·cancel handle·capabilities를 두시면 됩니다. 비밀번호·개인키 passphrase를 profile JSON에 직접 넣어서는 안 됩니다.

현재 welcome form과 protocol 선택은 `State` 전체에서 공유합니다. 여러 새 연결 탭의 입력을 독립적으로 보관하려면 session별 form draft가 필요합니다. terminal 설정도 전역 기본값 → profile override → 실행 중 값 순서로 정리하시는 것이 좋습니다.

### 8.2 공통 연결 정책을 추가하셔야 합니다

현재 프로토콜별 worker를 유지하고 timeout·retry·cancellation·trust 정책을 공통 타입으로 전달하시는 방향을 권장합니다. UI용 오류와 재시도 판단은 문자열 검색 대신 구조화된 원인으로 다루셔야 합니다. gateway는 주소를 바꾸는 옵션보다 transport 계층으로 구현하면 여러 프로토콜이 재사용할 수 있습니다.

SSH는 terminal·SFTP·forwarding이 채널을 생성할 수 있도록 handle 소유·종료·인증 경계를 설계하셔야 합니다. UI가 라이브러리 내부 handle을 직접 소유하기보다 service 명령으로 접근하게 하시는 것이 좋습니다. 재인증·agent forwarding 동의 범위도 구분하셔야 합니다.

### 8.3 terminal 코어의 직접 보완 여부를 판단하셔야 합니다

**확인:** 현재 `vte` 파싱 뒤 grid·mode·reflow·render semantics를 직접 구현하며 terminal 테스트는 없습니다.

**제안:** 직접 보완하면 기존 IME·Canvas를 유지하기 쉽지만 호환성을 계속 책임져야 합니다. 검증된 terminal core를 도입하면 mode·grid 처리를 재사용할 수 있으나 iced rendering·IME·selection adapter와 license 검토가 필요합니다. 특정 crate 채택은 이 조사에서 확정하지 않았습니다. 먼저 5.3절 회귀와 실제 TUI 요구사항을 기준으로 비교하시는 것이 좋습니다.

### 8.4 capability와 viewport를 명시하셔야 합니다

RDP secure attention처럼 이미 protocol별 구분이 있으므로 이를 clipboard·resize·audio·file transfer 등으로 확장하시는 것이 좋습니다. 지원하지 않는 기능은 UI에서 disabled 처리하고 이유를 표시하셔야 합니다. pane의 실제 bounds와 focus를 입력 정책에 전달하면 현재 고정 offset을 제거하고 분할 화면을 추가할 수 있습니다.

### 8.5 의존성과 배포 자료를 갱신하셔야 합니다

**확인:** `THIRD_PARTY_LICENSES.md`는 2026-03-25 기준이며 현재 Cargo.toml과 다릅니다. 특히 “serialport를 tokio-serial로 대체하여 제거했다”는 기록과 달리 Cargo.lock에는 `serialport` 4.9.0이 있습니다. 로컬 `tokio-serial` 5.4.5의 Cargo.toml은 serialport를 의존하며 serialport의 license는 MPL-2.0입니다. `deny.toml`은 MPL-2.0을 허용합니다.

따라서 대체로 인해 해당 의존성이 사라졌다는 기록은 수정해야 합니다. 이것만으로 배포가 불가능하다고 판단하지는 않았습니다. 실제 license 의무는 배포 구성별 검토가 필요합니다. X server·Unix 도구를 번들하면 현재 Rust 앱과 다른 배포 자료·고지가 필요할 수 있습니다.

## 9. 검증 결과와 남은 검증

### 9.1 이번에 실행한 검증

| 검증 | 결과 | 한계 |
|---|---|---|
| `cargo test --locked --offline` | 6 passed, 0 failed입니다. | 설정 persistence 4건과 State 2건입니다. 프로토콜·terminal·render 테스트는 없습니다. |
| 테스트 target 컴파일 | rustc/cargo 1.94.0에서 성공했습니다. | release 빌드·설치·GUI 실행은 검증하지 않았습니다. |
| compiler warning | `FieldFocused`, `RemoteDisplayInputs`가 생성되지 않는 dead_code 경고가 있습니다. | 현재 테스트 실패 원인은 아닙니다. |
| terminal 독립 재현 | ECH·경계 panic·alternate screen·한글 reflow 4건을 확인했습니다. | headless 코어 결과이며 실제 GUI 시나리오를 실행하지 않았습니다. |
| 의존성 코드 추적 | TLS verifier 우회와 resize의 DisplayControl 전제를 확인했습니다. | 실제 RDP 서버 handshake·resize는 실행하지 않았습니다. |
| 기존 문서와 비교 | VNC 협상·재시도, RDP resize wiring 등이 문서보다 진행되어 있습니다. | 문서의 과거 실서버 검증은 재실행하지 않았습니다. |

CI에는 license 검사와 태그 release 빌드가 있지만 PR에 대한 check/test/clippy 경로는 없습니다. `cargo deny`를 이번 조사에서 실행하지는 않았으므로 과거 pass 기록을 현재 결과로 취급하지 않았습니다.

### 9.2 실서버·실물 검증 계획

아래는 앞으로 수행할 완료 기준 제안입니다.

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

성능 기준은 같은 PC·서버·네트워크에서 MobaXterm과 비교하여 정하시는 것이 좋습니다. 이번 조사에는 측정값이 없으므로 FPS·RAM·지연의 우열을 주장하지 않았습니다.

## 10. 단계별 대체 목표

| 단계 | 사용자에게 제공할 결과 | 다음 단계로 넘어갈 조건 |
|---|---|---|
| A: 기본 신뢰·안정성 | 안전하게 연결하고 정상적으로 종료할 수 있습니다. | P0 항목과 terminal 재현 오류가 해결되고 회귀 검증이 있습니다. |
| B: 서버 관리 대체 | 저장 세션으로 key/MFA 로그인하고 파일을 옮기며 bastion·터널을 사용합니다. | profile·SSH 인증·SFTP·gateway·검색·출력 저장의 실제 업무 시나리오가 통과합니다. |
| C: 통합 작업 환경 | 분할 terminal과 검증된 RDP/VNC·원격 편집을 사용합니다. | 서버 matrix, clipboard·입력·resize·장애·성능 결과가 있습니다. |
| D: Linux GUI·Unix 환경 | X11 앱과 로컬 Unix 도구를 kterm에서 사용합니다. | X server·forwarding·도구 환경의 설치/portable 동작과 배포 조건을 만족합니다. |
| E: 전체 범위 확대 | 매크로·동시 실행·추가 프로토콜·내장 도구·기업 배포를 제공합니다. | 누락 기능별 지원 범위와 제한을 공개하고 사용 시나리오를 검증합니다. |

현재는 A단계의 선행 문제가 남아 있습니다. B단계는 여러 핵심 모듈을 새로 만들어야 하며, D·E단계는 별도 runtime·protocol·배포 과제를 포함합니다. 개발 인력과 검증 환경을 확인하지 않았으므로 완료 날짜는 추정하지 않았습니다.

## 11. 기존 문서 정리 대상

기존 문서를 덮어쓰지 않고 이번 분석을 추가했습니다. 이후 다음 기록을 실제 코드와 검증 상태에 맞추시는 것이 좋습니다.

- [README](../README.md): VNC Tight/ZRLE가 전부 미구현이라는 기록은 협상 구현과 JPEG 미처리로 나누어야 합니다. VNC 재시도는 구현되어 있으나 장애 검증·UX는 부족합니다. RDP resize는 이벤트 경로가 있고 DisplayControl wiring은 빠져 있습니다.
- [작업 목록](task.md): “코드 구현”, “실서버 검증”, “제품 사용 완료”를 다른 상태로 관리하셔야 합니다. 상위 미완 항목과 하위 완료 항목의 의미도 명확히 하셔야 합니다.
- [VNC 호환성 표](vnc_compatibility_matrix.md): 현재 TBD를 구현 완료 근거로 사용해서는 안 됩니다.
- [의존성 license 목록](../THIRD_PARTY_LICENSES.md): 현재 lockfile의 직접·간접 의존성을 기준으로 재생성하셔야 합니다.
- View/Help와 Theme placeholder는 기능을 구현하거나 사용 가능 기능으로 오인되지 않게 표시하셔야 합니다.

## 12. 주요 근거 위치

행 번호는 위 commit 기준이며 이후 변경되면 달라질 수 있습니다.

| 근거 | 위치 |
|---|---|
| protocol·runtime session 모델 | `src/app/model.rs:18`, `:88` |
| SSH 키 무조건 허용·password 인증·EOF | `src/connection/ssh.rs:18`, `:92`, `:35` |
| 연결별 설정 적용·입력·로그·종료 | `src/app/update.rs:15`, `:311` Connect 분기, `:527`, `:598` Paste 분기 |
| terminal bounds·reflow·CSI·debug I/O | `src/terminal.rs:223`, `:282`, `:450`, `:544`, `:801` |
| terminal·remote 입력 정책 | `src/app/subscription.rs:14`, `:201` |
| sidebar·Theme·dummy 메뉴 | `src/ui/view.rs:239`, `:588`, `src/ui/settings.rs:508` |
| 설정 저장 위치·동기 write | `src/app/settings_persistence.rs:288`, `:335` |
| RDP 종료·resize·GFX·TLS·domain | `src/connection/rdp.rs:329`, `:624`, `:652`, `:1077`, `:1130`, `:1230`, `:1245`, `:178` |
| VNC retry·TCP timeout·협상·clipboard·JPEG | `src/connection/vnc.rs:196`, `:300`, `:327`, `:752`, `:804` |
| PTY·Windows clipboard | `src/platform/windows.rs:34`, `:83` |
| version·feature·정확한 의존성 | `Cargo.toml`, `Cargo.lock` |
| 확인한 의존성 내부 구현 | Cargo registry `ironrdp-tls-0.2.0/src/rustls.rs:9`, `:61`; `ironrdp-session-0.8.0/src/active_stage.rs:219`; `tokio-serial-5.4.5/Cargo.toml:83` |

외부 비교 근거는 2.2절의 MobaXterm 공식 자료, terminal 동작 근거는 5.3절의 XTerm 문서입니다. 로컬 의존성 코드는 Cargo.lock에 지정된 버전을 읽었으며 최신 버전으로 대체하여 판단하지 않았습니다.
