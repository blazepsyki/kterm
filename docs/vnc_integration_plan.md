# VNC Integration Plan (MVP First)

## 목표
- 기존 RDP 경로와 공용 렌더링 경로를 재사용해 VNC 기능을 단계적으로 통합한다.
- 1차 목표는 MVP 범위(연결, 화면 표시, 기본 입력)로 제한한다.
- 렌더러 성능 최적화(dirty rect 전달/mark_clean)는 VNC 안정화 이후 별도 단계로 분리한다.

## 범위
- 포함
  - Welcome 탭에서 VNC 연결 입력(Host/Port/Password)
  - VNC 백엔드 연결 워커 추가
  - 공용 이벤트 계약 재사용: ConnectionEvent::Connected, Frames, Disconnected, Error
  - 공용 프레임 계약 재사용: FrameUpdate::Full, FrameUpdate::Rect
  - 기본 입력 매핑: 키보드(Unicode + 주요 스캔코드), 마우스 이동/버튼/휠
- 제외
  - 고급 인코딩 최적화(Tight/ZRLE 튜닝)
  - 커서 전용 렌더링(Cursor pseudo 반영)
  - SetDesktopSize/동적 리사이즈 확장

## 아키텍처 원칙
- src/connection/vnc.rs
  - VNC 프로토콜 차이를 흡수한다.
  - 내부에서 VNC 이벤트를 FrameUpdate로 변환해 UI에 전달한다.
- src/main.rs
  - 프로토콜 선택/세션 라우팅만 담당한다.
  - 렌더러는 기존 RemoteDisplayState 및 shader 파이프라인을 그대로 사용한다.
- src/remote_display/*
  - 프로토콜 독립 레이어로 유지한다.

## 단계별 계획
1. Phase 0: 인터페이스 정렬
- connection 모듈에 VNC 백엔드 노출
- 기존 공용 enum 계약 재사용

2. Phase 1: 연결/인증
- TCP 연결
- RFB 핸드셰이크
- 인증(None/VNC Auth 포함) 협상 경로 확보

3. Phase 2: 화면 이벤트 경로
- SetResolution -> FrameUpdate::Full
- RawImage -> FrameUpdate::Rect
- Frames 배치 이벤트를 UI로 전달

4. Phase 3: 입력 이벤트 경로
- RemoteInput::KeyboardUnicode/KeyboardScancode -> X11 KeyEvent
- RemoteInput::Mouse* -> X11 PointerEvent

5. Phase 4: UI 라우팅
- ProtocolMode::Vnc 추가
- VNC 입력 폼 및 ConnectVnc 핸들러 추가
- Session ID 기반 ConnectionMessage 라우팅 유지

6. Phase 5: 검증
- cargo check
- 실제 VNC 서버 연결 검증(로컬/원격)
- SSH/Telnet/Serial/Local/RDP 회귀 확인

## 현재 진행 상태 (2026-03-27)
- 완료
  - src/connection/mod.rs: VNC 모듈 노출 추가
  - src/main.rs: VNC 프로토콜 탭, 상태 필드, 메시지, ConnectVnc 라우팅 추가
  - src/connection/vnc.rs: 워커 스텁을 실구현 골격으로 교체
    - connect_and_subscribe 스트림 패턴(RDP와 동일한 채널 모델)
    - VNC 연결 + 이벤트 폴링 + 입력 처리 루프
    - FrameUpdate 변환(SetResolution/RawImage)
- 진행 중
  - 실제 서버별 인증/입력 호환성 미세 조정
  - 인코딩 확장 전략(Raw 우선 이후 Tight/ZRLE 검토)

## 구현 체크리스트
- [x] 모듈 노출 및 빌드 연결
- [x] Welcome VNC 폼 + Connect 동선
- [x] VNC 워커 이벤트 루프 기본 구현
- [x] Full/Rect 프레임 이벤트 연결
- [x] 기본 키/마우스 입력 송신
- [ ] 실제 서버 연결 테스트(최소 2종)
- [ ] 종료/예외 시나리오 회귀 테스트
- [ ] 문서(task.md) 진행 체크 반영

## 검증 명령
```powershell
cargo check
```

## 후속 확장
- Tight/ZRLE 인코딩 도입 및 성능 비교
- CursorPseudo 이벤트 반영(원격 커서 표시 정책)
- VNC Resize 정책(SetDesktopSize) 도입
- 렌더러 최적화 분리 작업(dirty_rects, mark_clean)
