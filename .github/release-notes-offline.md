<!-- TODO_COPY: release-notes-offline for the Slay the Spire 2 v0.113.0 build (Writer) -->
# pqp AI Guide (Offline) {{VERSION}}

**pqp AI Guide (Offline)** (mod ID `PqpAiGuideOffline`) is the standalone offline edition of the *Slay the Spire 2* AI guide mod. It is released exclusively here on GitHub Releases and is never published on the Steam Workshop.

## What It Does

- **In-game AI recommendations:** Shows pqp AI recommendation percentages (`%`) on cards, relics, potions, events, rest sites, shops, map paths, and combat choices.
- **Multiplayer AI teammates:** Lets the host add pqp AI teammates in multiplayer lobbies.
- **Play by Original Rules (`원본 규칙으로 플레이`):** Lets standard solo and multiplayer runs use original content pools and suspend game-logic patches from other mods without restarting.
- **Bundled, fixed AI model:** Includes a fixed AI policy model in `PqpAiGuideOffline/policy/` verified by SHA-256 before loading.

## What It Does Not Do

- **Never connects to the internet:** Contains no networking code and makes no network requests.
- **No recording or uploads:** Does not record runs, upload replays, or send training data.
- **No online UI or prompts:** Has no first-launch data-sharing consent popup, no in-game "내 리플레이" (My Replays) menu, no automatic model updates, and no external websites.

## Installation & Requirements

1. Download `{{ZIP}}` from this release and unzip it into `<game>\mods\` (your *Slay the Spire 2* folder's `mods` directory, creating `<game>\mods\PqpAiGuideOffline\`).
2. **Current Steam public beta build only:** This release works only on the current Steam public beta build of *Slay the Spire 2*. On any other game build, the mod turns itself off automatically.
3. **Use only one of the two editions:** Do not enable this mod together with [the online edition](https://steamcommunity.com/sharedfiles/filedetails/?id=3829382996) (**pqp AI Guide & Replay**). If the online edition is enabled, the offline edition automatically stands down and does nothing.

## Licenses

- **Source code (`AGPL-3.0-or-later`):** The mod's source code at https://github.com/moooohung/sts2-pqp-ai-guide is licensed under `AGPL-3.0-or-later` (see `LICENSE` and `LICENSE-EXCEPTION.md`).
- **Bundled AI model weights (`PqpAiGuideOffline/policy/MODEL-LICENSE.md`):** The AGPL-3.0 code license does not cover the model weights. Under `PqpAiGuideOffline/policy/MODEL-LICENSE.md`, the bundled weights are licensed solely for use inside this mod; extracting, modifying, redistributing separately, or training/distilling from them is prohibited.

*Not affiliated with Mega Crit.*

## Build & Checksums

- **Version:** `{{VERSION}}`
- **Archive:** `{{ZIP}}`
- **Archive SHA-256:** `{{ZIP_SHA256}}`
- **Bundled Model Checkpoint:** `{{CHECKPOINT_ID}}`
- **Model Weights SHA-256:** `{{WEIGHTS_SHA256}}`
- **Source Commit:** `{{SOURCE_COMMIT}}`

---

# 피큐피 AI 가이드 (오프라인) {{VERSION}} 안내

**피큐피 AI 가이드 (오프라인)** (`pqp AI Guide (Offline)`, 모드 ID `PqpAiGuideOffline`)은 *Slay the Spire 2*용 AI 가이드 모드의 오프라인 전용 에디션입니다. Steam 창작마당에는 출시되지 않으며, 오직 GitHub Releases를 통해서만 배포됩니다.

## 주요 기능

- **인게임 AI 추천 확률 표시:** 카드, 유물, 포션, 이벤트, 휴식 장소, 상점, 지도 경로 및 전투 선택지에 pqp AI 추천 확률(`%`)을 표시합니다.
- **멀티플레이 AI 동료:** 멀티플레이 로비에서 호스트가 조종하는 AI 동료를 추가해 함께 플레이할 수 있습니다.
- **원본 규칙으로 플레이:** 일반 싱글플레이 및 멀티플레이 런에서 게임을 재시작하지 않고도 원본 콘텐츠 풀을 사용하고 다른 모드의 게임 로직 패치를 일시 중지할 수 있습니다.
- **고정 내장형 AI 모델:** `PqpAiGuideOffline/policy/` 폴더에 고정된 AI 모델이 함께 포함되어 있으며, 로드 시 SHA-256 해시를 검증합니다.

## 포함되지 않는 기능 (완전 오프라인)

- **인터넷 연결 없음:** 네트워크에 전혀 연결하지 않습니다.
- **기록 및 업로드 없음:** 플레이 기록 저장, 리플레이 업로드, 학습 데이터 전송을 일절 수행하지 않습니다.
- **온라인 메뉴 및 동의 창 없음:** 첫 실행 데이터 제공 동의 창, 게임 내 ‘내 리플레이’ 메뉴, 모델 자동 업데이트, 외부 웹사이트 연결 기능이 없습니다.

## 설치 방법 및 주의사항

1. 이 릴리스에서 `{{ZIP}}` 파일을 내려받아 `<game>\mods\` 폴더(*Slay the Spire 2*가 설치된 폴더 안의 `mods` 폴더)에 압축을 풉니다(`<game>\mods\PqpAiGuideOffline\` 폴더가 생성됩니다).
2. **현재 Steam 공개 베타 빌드 전용:** 현재 Steam 공개 베타(`public-beta`) 빌드에서만 작동하며, 다른 게임 빌드에서는 모드가 자동으로 꺼집니다.
3. **두 에디션 중 하나만 사용:** 온라인 에디션(**pqp AI Guide & Replay / 피큐피 AI 가이드 & 리플레이**)과 동시에 켜 두지 마세요. 온라인 에디션이 활성화되어 있으면 오프라인 에디션은 자동으로 대기 상태가 되어 아무 동작도 하지 않습니다.

## 라이선스 안내

- **소스 코드 (`AGPL-3.0-or-later`):** 모드의 소스 코드(https://github.com/moooohung/sts2-pqp-ai-guide)는 `AGPL-3.0-or-later` 라이선스(`LICENSE` 및 `LICENSE-EXCEPTION.md`)가 적용됩니다.
- **내장 AI 모델 가중치 (`PqpAiGuideOffline/policy/MODEL-LICENSE.md`):** 소스 코드의 AGPL-3.0 라이선스는 모델 가중치에 적용되지 않습니다. `PqpAiGuideOffline/policy/MODEL-LICENSE.md`에 따라 모델 가중치는 본 모드 내부에서만 사용할 수 있으며, 별도 추출, 수정, 재배포 및 타 모델 학습·증류에 사용하는 행위는 금지됩니다.

*본 프로젝트는 Mega Crit과 제휴 관계가 아닙니다.*

