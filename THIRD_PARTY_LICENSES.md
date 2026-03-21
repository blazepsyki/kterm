# Third-Party Licenses

이 문서는 kterm이 사용하는 주요 크레이트의 라이선스 정보를 기록합니다.

- 기준: Cargo.lock, cargo metadata --locked
- 범위: 직접 의존성 + 라이선스 리스크 포인트
- 갱신 시점: 2026-03-21

## Direct Dependencies

| Crate | Version | License |
|---|---:|---|
| async-trait | 0.1.89 | MIT OR Apache-2.0 |
| bytes | 1.11.1 | MIT |
| crossbeam-channel | 0.5.15 | MIT OR Apache-2.0 |
| env_logger | 0.11.9 | MIT OR Apache-2.0 |
| futures | 0.3.32 | MIT OR Apache-2.0 |
| iced | 0.14.0 | MIT |
| log | 0.4.29 | MIT OR Apache-2.0 |
| nectar | 0.4.0 | MIT OR Apache-2.0 |
| portable-pty | 0.9.0 | MIT |
| russh | 0.58.0 | Apache-2.0 |
| russh-keys | 0.49.2 | Apache-2.0 |
| serialport | 4.9.0 | MPL-2.0 |
| tokio | 1.50.0 | MIT |
| tokio-util | 0.7.18 | MIT |
| unicode-width | 0.2.2 | MIT OR Apache-2.0 |
| vte | 0.15.0 | Apache-2.0 OR MIT |

## Notes On Potentially Sensitive Licenses

아래 항목은 메타데이터 상 GPL/LGPL/MPL 문자열이 보이는 경우입니다. 모두 즉시 충돌이라는 뜻은 아니며, OR 라이선스일 경우 permissive 옵션 선택이 가능합니다.

추가 참고 문서:

- [docs/MPL-2.0_GUIDE.md](docs/MPL-2.0_GUIDE.md)
- [docs/LICENSE-MPL-2.0](docs/LICENSE-MPL-2.0)

- serialport 4.9.0: MPL-2.0
- self_cell 1.2.2: Apache-2.0 OR GPL-2.0-only
- unescaper 0.1.8: GPL-3.0/MIT
- r-efi 5.3.0, 6.0.0: MIT OR Apache-2.0 OR LGPL-2.1-or-later

## CI Scan Status

- cargo deny check licenses: pass
- known warning: unescaper 0.1.8 uses deprecated SPDX style `GPL-3.0/MIT`
- impact: warning only, check exits with success

## Font Assets

### D2Coding.ttf

- upstream: https://github.com/naver/d2codingfont
- license: SIL Open Font License 1.1 (OFL-1.1)
- local license text: assets/fonts/OFL-1.1.txt
- local notice: assets/fonts/D2Coding-LICENSE-NOTICE.txt

공식 근거:

- D2Coding README의 라이선스 안내(OFL): https://github.com/naver/d2codingfont
- D2Coding Open Font License 위키(저작권/Reserved Font Name 포함): https://github.com/naver/d2codingfont/wiki/Open-Font-License
- OFL 1.1 공식 텍스트: https://openfontlicense.org/open-font-license-official-text/

## Regeneration

아래 명령으로 직접 의존성 라이선스 표의 원본 데이터를 다시 생성할 수 있습니다.

```powershell
$m = cargo metadata --format-version 1 --locked | ConvertFrom-Json
$root = $m.packages | Where-Object { $_.name -eq 'kterm' } | Select-Object -First 1
$depNames = $root.dependencies | ForEach-Object { $_.name } | Sort-Object -Unique
$depNames | ForEach-Object {
  $depName = $_
  $p = $m.packages | Where-Object { $_.name -eq $depName } | Sort-Object version -Descending | Select-Object -First 1
  [PSCustomObject]@{ Name = $p.name; Version = $p.version; License = $p.license }
} | Sort-Object Name
```

정확한 배포 컴플라이언스 문서는 릴리즈마다 CI 결과와 함께 재검토하세요.
