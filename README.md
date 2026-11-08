# pqp AI Guide & Replay (피큐피 AI 가이드 & 리플레이)

Source code for **pqp AI Guide & Replay**, an unofficial community mod and reinforcement learning AI engine for *Slay the Spire 2*.

- [English](#english)
- [한국어](#한국어)

---

## English

### Overview

**pqp AI Guide & Replay** ([Steam Workshop](https://steamcommunity.com/sharedfiles/filedetails/?id=3829382996)) brings the *Slay the Spire 2* reinforcement learning (PPO) AI from [sts.pqp.me](https://sts.pqp.me) into the game:

- **In-game AI recommendations**: displays the AI's action probability for each choice across card rewards, shops, events, the map, rest sites, and combat.
- **Multiplayer AI teammates**: adds `pqp AI` teammates in multiplayer lobbies and supports a "Play by Original Rules" mode.
- **Run recording & replays**: records Ascension 10 runs for replay viewing on [sts.pqp.me](https://sts.pqp.me) and for AI training (with opt-in consent and a separate replay-saving setting).

### Repository Layout

- `mod/`: the C# mod (Harmony patches, in-game UI, recording and upload queue, and localization strings in `mod/Localization/ui_strings.json`).
- `engine/`: the Rust source of the AI engine, which compiles to `sts2_advisor_bridge.dll` (a C ABI dynamic library loaded by the mod). Included crates: `advisor_bridge`, `aiplay`, `human_replay`, `native_obs`, `policy`, `sim`, `vecenv`, and `wire`, along with the patched `vendor/burn-optim-0.22.1`.

Without `sts2_advisor_bridge.dll`, the mod still loads normally and displays AI advice as unavailable.

### Workshop Release Source Commits

The Steam Workshop release was built from the following source commits:

- `mod`: `879a859ee7997a399e1122dcddd5da6b2d750de0` (`sts2_mods/HumanPlayRecorder`)
- `engine`: `1ffac8bf1ce96665cf0ebce3017767b0b8049d29` (`sts2_native`)

### Policy Weights

Trained policy weights (`*.safetensors`) are not stored in this repository. At run time, the mod fetches the current policy from [sts.pqp.me](https://sts.pqp.me) (or uses the copy bundled with the Workshop release) and verifies its SHA-256 hash and metadata before loading it.

### Building from Source

#### Build the Mod (`mod/`)

Requires the .NET SDK and an installed copy of *Slay the Spire 2* (game assemblies are not redistributed in this repository).

```bash
cd mod
dotnet build HumanPlayRecorder.csproj -c Debug -p:Sts2GameDir="<game install dir>"
```

- **Game directory property**: pass `-p:Sts2GameDir="<game install dir>"` or set the `STS2_GAME_DIR` environment variable to the directory containing `data_sts2_windows_x86_64` (for example, `G:\SteamLibrary\steamapps\common\Slay the Spire 2`). Alternatively, set the `GameData` MSBuild property directly to the `data_sts2_windows_x86_64` directory.
- **Release builds**: also pass `-p:RecorderSourceCommit=<40-hex commit>`.
- **Tests**:
  ```bash
  dotnet run --project tests/Tests.csproj -c Debug
  dotnet run --project tests/ContentEvidence/ContentEvidence.csproj -c Debug -p:Sts2GameDir="<game install dir>"
  ```

#### Build the AI Engine (`engine/`)

Requires the latest stable Rust toolchain (see `engine/rust-toolchain.toml`).

```bash
cd engine
cargo build --release -p sts2_advisor_bridge
cargo test --release --workspace
```

The built library is output to `engine/target/release/sts2_advisor_bridge.dll`. Place it next to `HumanPlayRecorder.dll`. Set the `STS2_ADVISOR_SOURCE_COMMIT` environment variable at build time to embed the commit hash reported by the library.

### What Is Not Included

This repository contains only the client-side mod and local inference/simulation engine. It does not include the training pipeline, trainer, supervisor, web dashboard and upload backend, recorded game captures and parity fixtures, policy weight files, decompiled game code, extracted game assets, or prebuilt Workshop binaries. See the initial commit message and `engine/EXPORT_NOTES.md` for the full list of excluded components.

### Data and Privacy

The mod records runs only with the player's consent: a prompt on first launch asks whether to opt in to AI-training data collection, and AI-training consent and replay saving can be toggled independently in settings at any time. When both settings are turned off, nothing is recorded or uploaded. Players can delete individual replays or all of their data directly in game. For full details on what is collected and how it is handled, see the [Privacy Policy](https://pqp.me/privacy/).

### License

- `mod/` and `engine/` are licensed under the **GNU Affero General Public License v3.0 or later** (`AGPL-3.0-or-later`); see [`LICENSE`](LICENSE).
- An **AGPL-3.0 Section 7 additional permission** grants permission to link and distribute the mod with the proprietary *Slay the Spire 2* game components, the built-in mod loader, and Harmony; see [`LICENSE-EXCEPTION.md`](LICENSE-EXCEPTION.md).
- `engine/vendor/burn-optim-0.22.1` is upstream Burn (`MIT OR Apache-2.0`) with a small local patch; its original license files are included in that directory.

### Disclaimer

This project is an unofficial community work and is not affiliated with, endorsed by, or sponsored by Mega Crit.

### Links and Contact

- Steam Workshop: https://steamcommunity.com/sharedfiles/filedetails/?id=3829382996
- AI Control Room & Replays: https://sts.pqp.me
- Privacy Policy: https://pqp.me/privacy/
- Terms of Service: https://pqp.me/terms/
- Contact: admin@pqp.me

---

## 한국어

### 소개

**피큐피 AI 가이드 & 리플레이 (pqp AI Guide & Replay)**([Steam 창작마당](https://steamcommunity.com/sharedfiles/filedetails/?id=3829382996))는 [sts.pqp.me](https://sts.pqp.me)에서 학습 중인 *Slay the Spire 2* 강화학습(PPO) AI를 게임 안에서 활용할 수 있게 해 주는 비공식 커뮤니티 모드이자 AI 엔진입니다.

- **게임 내 AI 추천 확률 표시**: 카드 보상, 상점, 이벤트, 지도, 휴식 장소, 전투 등 각 선택지마다 AI의 선택 확률을 표시합니다.
- **멀티플레이 AI 동료 및 원본 규칙 플레이**: 멀티플레이 로비에서 AI 동료(`pqp AI`)를 추가해 함께 달릴 수 있으며, '원본 규칙으로 플레이' 기능을 지원합니다.
- **런 기록 및 리플레이**: 승천 10 런을 기록하여 [sts.pqp.me](https://sts.pqp.me)에서 리플레이로 다시 보거나 AI 학습에 활용합니다(사전 동의 기반, 리플레이 저장 설정 분리).

### 저장소 구조

- `mod/`: C# 모드 소스 코드(Harmony 패치, 게임 내 UI, 기록 및 업로드 큐, `mod/Localization/ui_strings.json` 다국어 문자열).
- `engine/`: 모드가 로드하는 C ABI 동적 라이브러리 `sts2_advisor_bridge.dll`을 빌드하는 AI 엔진의 Rust 소스 코드. 포함 크레이트: `advisor_bridge`, `aiplay`, `human_replay`, `native_obs`, `policy`, `sim`, `vecenv`, `wire` 및 소폭 패치된 `vendor/burn-optim-0.22.1`.

`sts2_advisor_bridge.dll` 파일이 없더라도 모드는 정상적으로 로드되며, 게임 내에서 AI 추천만 사용 불가 상태로 표시됩니다.

### 창작마당 배포판 원본 커밋

Steam 창작마당 배포판은 아래 원본 커밋에서 빌드되었습니다.

- `mod`: `879a859ee7997a399e1122dcddd5da6b2d750de0` (`sts2_mods/HumanPlayRecorder`)
- `engine`: `1ffac8bf1ce96665cf0ebce3017767b0b8049d29` (`sts2_native`)

### 정책 가중치 (모델 파일)

학습된 정책 가중치 파일(`*.safetensors`)은 이 저장소에 포함되어 있지 않습니다. 모드는 실행 시점에 [sts.pqp.me](https://sts.pqp.me)에서 최신 정책 가중치를 받아오거나 창작마당 배포판에 동봉된 사본을 사용하며, 로드하기 전에 SHA-256 해시와 메타데이터를 검증합니다.

### 빌드 방법

#### 모드 빌드 (`mod/`)

.NET SDK와 설치된 *Slay the Spire 2* 게임이 필요합니다(게임 어셈블리는 저장소에 포함되지 않습니다).

```bash
cd mod
dotnet build HumanPlayRecorder.csproj -c Debug -p:Sts2GameDir="<game install dir>"
```

- **게임 경로 속성**: `data_sts2_windows_x86_64` 폴더가 들어 있는 게임 설치 경로(예: `G:\SteamLibrary\steamapps\common\Slay the Spire 2`)를 `-p:Sts2GameDir="<game install dir>"` 빌드 속성 또는 `STS2_GAME_DIR` 환경 변수로 지정합니다. `GameData` 속성에 `data_sts2_windows_x86_64` 폴더 경로를 직접 지정할 수도 있습니다.
- **Release 빌드**: `-p:RecorderSourceCommit=<40자리 16진수 커밋 해시>` 속성을 함께 전달해야 합니다.
- **테스트 실행**:
  ```bash
  dotnet run --project tests/Tests.csproj -c Debug
  dotnet run --project tests/ContentEvidence/ContentEvidence.csproj -c Debug -p:Sts2GameDir="<game install dir>"
  ```

#### AI 엔진 빌드 (`engine/`)

최신 안정 버전의 Rust 툴체인이 필요합니다(`engine/rust-toolchain.toml` 참고).

```bash
cd engine
cargo build --release -p sts2_advisor_bridge
cargo test --release --workspace
```

빌드된 라이브러리는 `engine/target/release/sts2_advisor_bridge.dll`에 생성됩니다. 이 파일을 `HumanPlayRecorder.dll`과 같은 폴더에 배치하면 됩니다. 빌드 시 `STS2_ADVISOR_SOURCE_COMMIT` 환경 변수를 설정하면 라이브러리가 보고하는 커밋 해시를 기록할 수 있습니다.

### 포함되지 않는 항목

이 저장소에는 클라이언트 모드와 로컬 추론·시뮬레이션 엔진만 포함됩니다. 학습 파이프라인, 트레이너, 슈퍼바이저, 대시보드 및 업로드 도구, 기록된 게임 캡처 및 정합성 검증 픽스처, 정책 가중치, 디컴파일된 게임 코드, 추출된 게임 에셋, 패키징된 창작마당 바이너리는 제외되어 있습니다. 제외된 항목의 상세 목록은 커밋 메시지와 `engine/EXPORT_NOTES.md`를 참고해 주세요.

### 데이터 및 개인정보

모드는 플레이어의 동의가 있을 때만 런을 기록합니다. 첫 실행 시 AI 학습 데이터 제공 여부를 묻는 동의 창이 표시되며, 모드 설정에서 AI 학습 동의와 리플레이 저장 설정을 언제든 개별적으로 켜고 끌 수 있습니다. 두 설정을 모두 끄면 어떤 데이터도 기록되거나 업로드되지 않습니다. 또한 게임 안에서 자신의 데이터를 직접 삭제할 수 있습니다. 자세한 내용은 [개인정보처리방침](https://pqp.me/privacy/)을 확인해 주세요.

### 라이선스

- `mod/`와 `engine/`은 **GNU Affero General Public License v3.0 또는 이후 버전**(`AGPL-3.0-or-later`)으로 배포됩니다([`LICENSE`](LICENSE)).
- 독점 소프트웨어인 *Slay the Spire 2* 게임 본체 및 모드 로더·Harmony와 링크하여 배포할 수 있도록 **AGPL 제7조에 따른 추가 허가(예외 조항)**가 적용됩니다([`LICENSE-EXCEPTION.md`](LICENSE-EXCEPTION.md)).
- `engine/vendor/burn-optim-0.22.1`은 업스트림 Burn(`MIT OR Apache-2.0`)에 소규모 패치를 적용한 사본으로, 해당 폴더 안에 원본 라이선스 파일이 함께 포함되어 있습니다.

### 면책 및 비제휴 안내

이 프로젝트는 비공식 커뮤니티 저작물이며, Mega Crit과 제휴·승인·후원 관계가 아닙니다.

### 링크 및 문의

- Steam 창작마당: https://steamcommunity.com/sharedfiles/filedetails/?id=3829382996
- AI 관제실 및 리플레이: https://sts.pqp.me
- 개인정보처리방침: https://pqp.me/privacy/
- 이용약관: https://pqp.me/terms/
- 문의: admin@pqp.me

