# Redrob Board

레드롭 보드는 말로 한 사양을 공장에 바로 넘길 수 있는 PCB로 만드는 에이전트 기반 PCB 설계 스튜디오입니다.

English: [README.md](./README.md)

## 무엇인가

Rust로 쓴 회로에서 거버·드릴·BOM·픽앤플레이스 파일까지, 한 Rust 워크스페이스로 처음부터 만들었습니다.
에이전트가 모든 단계를 움직입니다.

- **board-units** -- 정수 나노미터 길이. 지오메트리 판정에 부동소수를 쓰지 않습니다.
- **board-geom** -- 정확한 점·사각형·선분·폴리곤.
- **board-netlist** -- 모든 도구가 나눠 쓰는 중간 표현. 문제를 한 번에 전부 보고하는 검증.
- **board-schematic** -- 회로도는 Rust 코드입니다. 부품을 넣고 핀을 이으면 넷리스트가 나옵니다.
  그림 회로도는 그 넷리스트의 뷰일 뿐, 원본이 아닙니다.
- **board-layout** -- 보드 자체: 레이어 스택, 외곽, 배치된 풋프린트, 트랙, 비아.
- **board-gerber** -- RS-274X 쓰기·읽기. CI가 모든 내보내기의 왕복 일치를 증명합니다.
- 부품 라이브러리, DRC, 배치, push-and-shove 라우터, 드릴/BOM/CPL, 회로 시뮬, egui 앱, 에이전트
  도구가 각자 크레이트로 뒤따릅니다([docs/ROADMAP.ko.md](docs/ROADMAP.ko.md)).

레드롭 실리콘은 칩 안을, 레드롭 보드는 칩이 앉는 보드를 설계합니다. 둘은 한 흐름(넷리스트 -> 배치 ->
배선 -> 검사 -> 제조 파일)을 가지며 지오메트리·DRC·뷰어 코드를 나눠 씁니다.

JavaScript 없음, 웹뷰 없음, 외부 EDA 바이너리 없음. 상류 도구(KiCad, ngspice)는 CI 시험지로만 씁니다.
우리 DRC 위반 수와 거버 렌더가 그것과 일치해야 단계 완료입니다. 제품에는 들어가지 않습니다.
[UPSTREAM.md](./UPSTREAM.md) 참고.

## 소스 빌드

```sh
cargo test --workspace
cargo run -p board-agent --bin board-cli -- pipeline --design sensor-node --outline 30x25 --edge J1=left --edge SW1=right --out fab
cargo run -p redrob-board         # 데스크톱 앱
```

## 상태

0단계(부트스트랩)·1단계(풋프린트 라이브러리, DRC)·2단계(배치)·3단계(배선)·4단계(제조 파일)·5단계(앱) 완료. `oracle` 워크플로가 예제 보드를 KiCad로
내보내 KiCad DRC 위반 수와 우리 수가 같아야 통과합니다: 간격·트랙 폭·고리·드릴·단락·미연결이 앞뒷면 예제 넷에서
일치. 배치 엔진은 14부품 센서 노드를 겹침 없이, 손 배치보다 배선 길이 54% 짧게 놓습니다(게이트 20%). 라우터는 분배기와 센서 노드
두 배치를 2층으로 끝까지 배선해 DRC 위반 0이고, KiCad도 배선된 센서 노드를 깨끗하다고 판정합니다. 우리 거버(동박·마스크·페이스트·외곽)와 드릴 파일은
같은 보드를 KiCad가 그린 것과 우리 리더로 10um 래스터화해 비교합니다: 42개 파일 전부 픽셀 일치. BOM·픽앤플레이스
CSV는 `board-bom`이 냅니다. `board-cli pipeline --design sensor-node --outline 30x25 --edge J1=left --edge SW1=right --out fab`이
전체 흐름을 돌려 배선 완주·DRC 0일 때만 종료 코드 0을 냅니다. `redrob-board`(egui) 앱은 같은 일을 버튼으로 하고,
헤드리스 `egui_kittest` 테스트가 그 버튼들을 눌러 렌더 결과를 `crates/app/tests/snapshots/sensor_node_routed.png`와 비교합니다.
센서 노드는 ESP32-C3-MINI-1과 AHT20의 데이터시트 랜드 패턴과 핀 번호를 씁니다(모듈 풋프린트는 Espressif 공식 KiCad
풋프린트와 패드 단위로 대조). 단계와 기계적 통과 조건은 [docs/ROADMAP.ko.md](docs/ROADMAP.ko.md)에 있습니다.

## 설치

릴리즈에는 서명·공증된 macOS 디스크 이미지(universal), 서명된 Windows 설치 파일과 포터블 zip(x64·arm64),
Linux AppImage/deb/rpm/tar.gz(x86_64·aarch64)가 있습니다:
[github.com/redrob-labs/redrob-board/releases](https://github.com/redrob-labs/redrob-board/releases).
`vX.Y.Z` 태그가 `.github/workflows/release.yml`을 돌려 한 커밋에서 모든 잡을 빌드하고 드래프트 릴리즈를 만듭니다.
패키징 스크립트는 `packaging/`에 있습니다.

## 라이선스

MIT 또는 Apache-2.0 중 선택. Copyright 2026 Janghoon Lee (Redrob) and contributors.
