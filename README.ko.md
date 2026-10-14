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
```

## 상태

0단계(부트스트랩) 완료: units, geom, netlist, schematic eDSL, 보드 모델, 자기 리더로 왕복되는 거버
라이터. 단계와 기계적 통과 조건은 [docs/ROADMAP.ko.md](docs/ROADMAP.ko.md)에 있습니다.

## 라이선스

MIT 또는 Apache-2.0 중 선택. Copyright 2026 Janghoon Lee (Redrob) and contributors.
