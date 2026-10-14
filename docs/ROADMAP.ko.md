# Redrob Board 로드맵 (초안 v1, 2026-10-15)

## 목표

레드롭 하드웨어 제품(스마트홈 기기, 로봇, NPU 브링업 보드)의 PCB를 에이전트가 설계한다.
요구사항 한 문장 -> 회로도 -> 부품 선택 -> 배치 -> 배선 -> DRC -> 거버·BOM·픽앤플레이스 -> 공장 발주.

Redrob Silicon이 칩 안을, Redrob Board가 칩 사이를 맡는다. 둘은 같은 흐름(넷리스트 -> 배치 -> 배선 ->
검사 -> 제조 파일)을 가지므로 지오메트리·DRC·캔버스 코드를 나눠 쓴다.

| 산출물 | 무엇 | 왜 |
|---|---|---|
| Redrob Board (도구) | 에이전트가 붙은 PCB 설계 스튜디오. 전부 우리 코드 | 부품 선택·배치·배선을 AI가 넓게 탐색해야 보드 품질이 나온다 |
| 레드롭 보드 라이브러리 | 심볼·풋프린트·가격·재고를 코드로 | 에이전트가 BOM 비용·재고까지 보고 고른다 |
| 첫 보드 | NPU 브링업 보드(Silicon 6단계), 스마트홈 센서 노드 | 우리 제품이 첫 손님 |

## 원칙: build the hard way

- 전부 Rust. JS/TS 없음. Tauri·Electron·webview 없음. 외부 EDA 바이너리 호출 없음.
- 포크 아님. Rust로 쓸 만한 PCB 도구가 없으므로 클린룸 재구현. 상류(KiCad, LibrePCB, Horizon EDA,
  Freerouting, tscircuit, atopile)는 읽고 배우되 코드는 가져오지 않는다.
- 회로도도 코드다. Rust 안에 심은 eDSL(`board-schematic`)로 회로를 쓰고, 그 자료구조가 넷리스트다.
  그림 회로도는 그 넷리스트를 자동 배치해 그린 뷰일 뿐이다.
- 설계 파일은 전부 텍스트. git diff 가능.
- 상류 도구는 시험지로만. KiCad DRC·거버 뷰어 결과와 CI에서 비교한다. 제품에 들어가진 않는다.
- 기계적 게이트만 통과로 친다. 종료 코드.

## 크레이트

| 크레이트 | 하는 일 | 상류 참고(코드 아님) |
|---|---|---|
| board-units | 나노미터 정수 좌표, mm/mil 변환. 부동소수 금지 | KiCad(nm 정수) |
| board-geom | 점·선분·호·폴리곤·사각형. 간격·교차·오프셋 | Clipper2 알고리즘, KiCad geometry |
| board-netlist | 부품·핀·넷·풋프린트 참조. 직렬화(우리 포맷) + KiCad 넷리스트 export | KiCad s-expr |
| board-schematic | Rust eDSL. 부품 인스턴스·연결·버스. 빌드하면 넷리스트 | atopile, tscircuit |
| board-lib | 풋프린트(패드·실크·코트야드)·심볼·부품 메타(가격·재고·대체품). KiCad 풋프린트 import | KiCad .kicad_mod |
| board-layout | 보드 자료구조: 레이어 스택, 외곽, 부품 배치, 트랙·비아·존 | KiCad pcbnew |
| board-drc | 간격·폭·고리·코트야드·미연결·단락. 위반 목록을 종료 코드로 | KiCad DRC |
| board-place | 부품 배치. 힘기반 + 시뮬레이티드 어닐링. 신호 길이·열·커넥터 가장자리 | — |
| board-route | 격자 없는 push-and-shove 라우터. 차동쌍·길이 맞춤 | Freerouting, KiCad PNS |
| board-gerber | RS-274X 거버, Excellon 드릴, IPC-2581 export | Ucamco 스펙 |
| board-bom | BOM CSV, 픽앤플레이스 CPL, JLCPCB·PCBWay 포맷 | — |
| board-sim | SPICE급 회로 시뮬(선형 + 단순 비선형). 전원·바이어스 검증 | ngspice(CI 오라클) |
| board-render | wgpu 레이어 뷰어, 3D 보드 미리보기 | — |
| board-agent | 엔진 도구 정의: write_schematic, pick_parts, place, route, drc, export, order | redrob-engine-client |
| board-app | egui/eframe 앱. 웹 빌드는 wasm | cadcraft·launcher 구조 |

## 단계

| # | 이름 | 하는 일 | 통과 조건 |
|---|---|---|---|
| 0 | 부트스트랩 | 레포, org 템플릿, 워크스페이스, CI(2 OS). units·geom·netlist·schematic·layout 뼈대. 외곽+패드만 있는 보드를 거버로 | `cargo test` 초록, 거버 파서 왕복 일치 |
| 1 | 라이브러리 + DRC | board-lib(KiCad 풋프린트 import), board-drc. 예제: ESP32-C3 센서 노드 회로 | KiCad DRC 위반 수와 일치 |
| 2 | 배치 | board-place. 커넥터 가장자리·열 분리·HPWL | 수동 배치 대비 HPWL -20% 이상 |
| 3 | 배선 | board-route 단층·양층. 미연결 0 | DRC 0, 완주율 100% (예제 3종) |
| 4 | 제조 파일 | gerber·drill·bom·cpl. JLCPCB 포맷 | KiCad 거버 뷰어 렌더와 픽셀 일치 |
| 5 | 앱 MVP | board-app + agent 7도구 + render | 채팅 "ESP32 온습도 센서 노드" -> 거버 ZIP 자동, egui_kittest 스냅샷 |
| 6 | 첫 발주 | 센서 노드 보드 JLCPCB 발주, 조립 | 보드 켜지고 펌웨어(redrob-code) 올라감 |
| 7 | NPU 브링업 보드 | Silicon 6단계용. 4층, 전원 시퀀스, 디버그 | 칩 소켓·JTAG 동작 |
| 8 | 시뮬 + 4층 이상 | board-sim, 차동쌍, 임피던스, 내층 | ngspice 오차 1%, 4층 DRC 0 |
| 9 | 발주 자동화 | JLCPCB·PCBWay API, 재고 기반 부품 교체 | 채팅 -> 주문 번호 |

0->1->2->3->4 순서 고정. 5는 2부터 병행. 6은 4 뒤. 7은 6 뒤. 가장 큰 덩어리: board-route.
6 이후는 돈과 실물이 필요하다. 거기서 보고하고 멈춘다.

## 레드롭 랩스 규칙

- 가짜 문 없음. 보이는 버튼은 뒤가 있다. 발주 API는 동작 전까지 Labs 스위치 뒤.
- org 템플릿: README.md/README.ko.md, UPSTREAM.md(참고 상류 목록, 코드 미사용 명시), gitflow.yml.
- 릴리즈: redrob-launcher와 같은 Rust release.yml(3 OS, OIDC 서명). 데몬 없으면 런처 안내.
- description: "Redrob Board is an agentic PCB design studio that takes a spoken spec to a factory-ready board."
- 상용 EDA·경쟁사 언급 금지. 상류 귀속은 UPSTREAM.md에만.
- 제품색 없음(디자인 시스템 20-color.md). 중립 표면 + Redrob Blue.

## 돈이 드는 지점

| 항목 | 대략 | 단계 |
|---|---|---|
| 센서 노드 보드 5장 + 조립 | 수만 원 | 6 |
| NPU 브링업 보드 4층 + 조립 | 수십만 원 | 7 |
| 부품 재고·가격 API(LCSC·Mouser) | 무료 티어 | 9 |

## 아직 모르는 것

- push-and-shove 라우터를 Rust로 DRC 0까지 끌고 가는 반복 수.
- KiCad 풋프린트 라이브러리 라이선스(CC-BY-SA 4.0 + 예외) 하에서 import 결과를 어떻게 배포하나.
- Silicon의 geom·drc와 어디까지 한 크레이트로 합칠 수 있나.
