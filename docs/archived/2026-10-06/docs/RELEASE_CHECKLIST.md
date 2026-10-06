# Release Checklist

## Licensing

- [ ] Cargo.lock 최신화 후 라이선스 스캔 통과 확인
- [ ] GitHub Actions License Check 워크플로 통과
- [ ] cargo-deny 경고 항목(현재 unescaper SPDX parse-warning) 검토
- [ ] THIRD_PARTY_LICENSES.md 갱신
- [ ] LICENSE, LICENSE-MIT, LICENSE-APACHE 파일 존재 확인
- [ ] assets/fonts/OFL-1.1.txt 및 assets/fonts/D2Coding-LICENSE-NOTICE.txt 존재 확인
- [ ] Cargo.toml의 license 필드가 MIT OR Apache-2.0인지 확인
- [ ] 소스 상단 SPDX 헤더 일관성 확인

## Security/Compliance

- [ ] SSH host key 검증 정책 점검
- [ ] Telnet 기능 문서의 평문 경고 최신화
- [ ] 외부 바이너리/리소스의 재배포 조건 확인

## Build/Quality

- [ ] cargo fmt --check
- [ ] cargo clippy --all-targets --all-features -D warnings
- [ ] cargo test
- [ ] cargo build --release

## Release Metadata

- [ ] README 기능/제약/실행법 최신화
- [ ] 변경 로그(태그 노트) 작성
- [ ] 버전 태그와 Cargo.toml 버전 일치 확인
