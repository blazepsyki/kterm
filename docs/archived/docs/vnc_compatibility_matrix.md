# VNC Compatibility Matrix (Tight/ZRLE)

## 목적
- Tight/ZRLE 협상 적용 이후 서버별 호환성/지연 특성을 동일 시나리오로 비교한다.
- 최종 인코딩 우선순위 정책(ZRLE 우선 vs Tight 우선)을 데이터 기반으로 확정한다.

## 공통 테스트 조건
- 클라이언트: kterm (branch/head 기준)
- 빌드: debug/release 명시
- 네트워크: 로컬/LAN/WAN 프로파일 구분
- 화면 시나리오:
  - Idle 60s
  - 텍스트 고속 스크롤 60s
  - 고변화 화면(영상/애니메이션) 120s

## 수집 항목
- 협상 인코딩 후보/결과
- SetPixelFormat 로그 (bpp/depth/shifts)
- 메트릭 로그: ticks/events/full/rect/forced_full_batches/jpeg_events/rect_only_streak
- 입력-화면 반응 지연: p50/p95/p99
- 프레임 품질: 깨짐/잔상/색왜곡 여부
- 복구 동작: 단절 시 자동 재시도 성공 여부

## 서버 매트릭스

| Server | OS | Auth | Negotiated Encoding | PixelFormat | p50(ms) | p95(ms) | p99(ms) | JPEG Events | Artifacts | Auto-Reconnect | Result |
|---|---|---|---|---|---:|---:|---:|---:|---|---|---|
| TigerVNC | Linux | Password | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| x11vnc | Linux | Password | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| TightVNC | Windows/Linux | Password | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| UltraVNC | Windows | Password | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| RealVNC (optional) | Mixed | Password | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |

## 단계별 체크리스트

### 1) 연결/협상
- [ ] 서버 연결 성공
- [ ] 인증 성공(None/Password)
- [ ] 협상 로그에 ZRLE/Tight 후보 노출
- [ ] 협상 실패 시 오류 사유 분류 확인

### 2) 화면 정확도
- [ ] 초기 전체 화면 정상
- [ ] 부분 업데이트(Rect) 정상
- [ ] CopyRect 동작 정상
- [ ] 커서 렌더(CursorPseudo) 정상
- [ ] 색상 왜곡/텍스트 번짐 없음

### 3) 입력/지연
- [ ] 키보드 입력/조합키 정상
- [ ] 마우스 이동/버튼/휠 정상
- [ ] 잠금키 동기화(Caps/Num/Scroll) 정상
- [ ] 지연 p95가 Raw 기준 대비 허용 범위 내

### 4) 복구/안정성
- [ ] 네트워크 단절 후 자동 재시도 동작
- [ ] 재연결 후 프레임/입력 회복
- [ ] 재시도 소진 시 에러 메시지 가시성 확인

## 정책 결정 가이드
- ZRLE 우선 조건:
  - 지연 p95가 Tight 대비 안정적이거나 우수
  - JPEG 이벤트 의존 없이 화면 품질이 안정적
- Tight 우선 조건:
  - 지연 악화 없이 대역폭 절감 효과가 유의미
  - JPEG 이벤트 발생이 적고 아티팩트 없음
- 서버 프로파일 분기 조건:
  - 특정 서버에서만 반복되는 아티팩트/지연 회귀가 재현됨

## 실행 로그
- 2026-03-31: 템플릿 생성. 실측 데이터 미입력(TBD).
