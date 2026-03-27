# VNC Integration Plan (Full Track)

## 목표
- 기존 RDP 경로와 공용 렌더링 경로를 재사용해 VNC를 실사용 가능한 Full 품질로 확장한다.
- MVP 착수분 위에서 보안/인코딩/입력/수명주기/검증을 단계적으로 강화한다.
- UI/렌더러는 공용 계약(FrameUpdate)만 소비하고, 프로토콜 세부는 VNC 백엔드가 흡수한다.

## 범위
- 포함
  - 연결/인증 안정화 (오류 분류, 타임아웃, 사용자 피드백)
  - 화면 경로 완성 (Full/Rect 일관성 + 부분 업데이트 검증)
  - 입력 고도화 (keysym 커버리지, 수정자/락키 정책)
  - 수명주기 안정화 (탭 종료, 서버 종료, 단절 처리)
  - 서버 호환성 매트릭스 기반 검증
- 선택(Full+)
  - 클립보드 양방향
  - CursorPseudo 렌더링
  - 동적 해상도(SetDesktopSize)

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
  - 인코딩 확장 전략(Raw 우선 이후 ZRLE/Tight 검토)

## 단계별 계획 (병합본)
1. Phase A - 기준선 고정 (현재 상태 확정)
- 이미 반영된 VNC UI/워커 코드를 기준선으로 고정하고, Full 범위 완료 기준을 문서화한다.

2. Phase B - 인증/연결 안정화
- src/connection/vnc.rs에서 보안타입 협상과 인증 오류 분류를 강화한다.
- connect timeout 및 초기 실패 메시지 표준화를 적용한다.

3. Phase C - 프레임 경로 완성
- VNC 이벤트(SetResolution, RawImage, Error)에서 FrameUpdate 변환 계약을 정교화한다.
- 전체/부분 프레임 처리 일관성을 보장한다.

4. Phase D - 렌더러 실제 최적화 활성화
- main.rs 뷰에서 dirty_rects/full_upload 플래그를 실제 사용하도록 연결한다.
- remote_display mark_clean 호출 시점을 정의해 부분 업로드 경로를 활성화한다.

5. Phase E - 입력 정확도 고도화
- VNC keysym 매핑 커버리지를 확장한다.
- 락키/수정자 해제/휠 처리 정책을 서버 호환성 중심으로 정리한다.

6. Phase F - 좌표/스케일 일관화
- transform_remote_mouse와 셰이더 화면비 보정 중복으로 인한 오차를 줄인다.
- 클릭 좌표 일치율 기준으로 정책을 고정한다.

7. Phase G - 수명주기/오류 복구
- 탭 닫기, 서버 종료, 네트워크 일시 실패를 분리 처리한다.
- Disconnected/Error 전파와 워커 정리를 안정화한다.

8. Phase H - 인코딩 전략 확장
- Raw 경로 안정화 후 서버 호환성 기반 인코딩 우선순위(ZRLE/Tight/Raw fallback)를 적용한다.
- 성능 로그를 도입한다.

9. Phase I - 기능 확장(선택)
- 클립보드 양방향, CursorPseudo, 동적 해상도(SetDesktopSize)를 Full+ 범위로 단계 적용한다.

10. Phase J - 문서/체크리스트 동기화
- docs/vnc_integration_plan.md를 실행 로그 중심 문서로 유지한다.
- docs/task.md 14단계와 docs/rdp_integration_plan.md 공용화 항목을 결과와 동기화한다.

11. Phase K - 게이트 기반 검증
- 자동 검증(cargo check, clippy, deny, release build)과 수동 검증을 통과해야 다음 단계로 진행한다.

## Parallelism and Dependencies
1. Phase D(렌더러)와 Phase E(입력)는 Phase B 완료 후 병렬 진행 가능.
2. Phase H(인코딩 확장)는 Phase G 안정화 직후 병렬 실험 가능.
3. Phase B(보안/연결 안정화) 없이 성능/고급 기능 단계로 넘어가지 않는다.
4. 각 Phase 종료 시 게이트 통과 전 다음 Phase 착수 금지.

## Relevant files
- d:/Downloads/Rust_dev/kterm/src/connection/vnc.rs - 연결/인증, 이벤트 루프, 입력/프레임 변환 핵심.
- d:/Downloads/Rust_dev/kterm/src/main.rs - ProtocolMode/ConnectVnc 라우팅, RemoteDisplay 뷰 연결.
- d:/Downloads/Rust_dev/kterm/src/remote_display/mod.rs - FrameUpdate 적용, dirty_rects/full_upload 상태.
- d:/Downloads/Rust_dev/kterm/src/remote_display/renderer.rs - 부분 텍스처 업로드 및 GPU 렌더링.
- d:/Downloads/Rust_dev/kterm/src/connection/remote_input_policy.rs - 공용 키 라우팅 정책.
- d:/Downloads/Rust_dev/kterm/src/connection/mod.rs - 공용 이벤트/입력 계약 및 모듈 노출.
- d:/Downloads/Rust_dev/kterm/docs/task.md - 14단계 진행 상태.
- d:/Downloads/Rust_dev/kterm/docs/rdp_integration_plan.md - RDP/VNC 공용 정책 정합성.

## Verification
1. 자동 검증: cargo check 통과.
2. 자동 검증: cargo clippy --lib -- -D warnings 통과 또는 기존 허용 경고만 유지.
3. 자동 검증: cargo deny check 통과.
4. 자동 검증: cargo build --release 통과.
5. 수동 기능 검증: TightVNC 기준 연결/인증(None, Password)/초기화면/Rect/입력(키보드,마우스,휠) 확인.
6. 수동 호환성 검증: 최소 2종 추가 서버(예: RealVNC/Proxmox 계열) 확인.
7. 수동 수명주기 검증: 탭 닫기, 서버 종료, 네트워크 단절 후 이벤트/리소스 정리 확인.
8. 회귀 검증: SSH, Telnet, Serial, Local, RDP 스모크 테스트.
9. 성능 검증: Full upload 대비 Rect 경로의 체감 지연/업데이트 빈도 개선 확인.

## Go-NoGo Gates
1. Gate 1 (연결 안정화): 실패 사유 식별 가능 + 성공 연결 재현 가능.
2. Gate 2 (렌더 정확도): 왜곡/좌표 불일치 없이 Full/Rect 안정 동작.
3. Gate 3 (입력 안정성): 주요 키/마우스/수정자 해제가 일관 동작.
4. Gate 4 (회귀): 비-VNC 프로토콜 기능 저하 없음.
5. Gate 5 (출시 준비): 호환성 매트릭스와 문서 동기화 완료.

## Decisions
- 포함 범위: VNC Full 구현(연결/인증/화면/입력/수명주기/성능 검증).
- 포함 범위: dirty_rects 경로 실제 사용을 위한 렌더러 연계 보완.
- 선택 범위(Full+): 클립보드 양방향, CursorPseudo, 동적 해상도.
- 제외 범위: 16단계 이전 범용 네트워크 복구 프레임워크 대규모 재설계.
- 설계 원칙: UI/렌더러는 FrameUpdate 계약만 소비, 프로토콜 세부는 src/connection/vnc.rs에서 흡수.

## 작업 시작 로그 (2026-03-27)
- [시작] Phase B(인증/연결 안정화) 착수
  - 연결 타임아웃 적용
  - 인증 관련 사용자 경고 메시지 강화
  - 초기 전체 프레임 요청(FullRefresh) 명시 전송
- [시작] Phase D(렌더러 최적화) 착수
  - RemoteDisplayState에 frame_seq 도입 및 증분 업데이트 전환 로직 추가
  - main.rs 셰이더 Program에 dirty_rects/full_upload/frame_seq 실배선
  - renderer 파이프라인에 last_uploaded_seq 추적을 추가해 동일 프레임 중복 업로드 방지
- [시작] Phase E(입력 안정화) 착수
  - ConnectionInput::SyncKeyboardIndicators를 VNC 경로에서 무시하지 않고 처리
  - Caps/Num/Scroll Lock 상태 변화 시 keysym 토글(press/release) 전송
  - 첫 동기화 시에는 baseline만 설정하고 이후 변화분만 반영
