# RDP 통합 구현 계획 (IronRDP)

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
- 보안 정책(TLS/NLA/인증서 검증)은 기본 안전 설정을 우선한다.
- 성능 최적화는 full-frame 동작을 먼저 완성한 다음 dirty-rect로 확장한다.
- `russh 0.57+` 계열에서는 `sha1` 프리릴리스 충돌이 재발할 수 있으므로, `russh` 업그레이드는 별도 검증 브랜치에서 수행한다.
