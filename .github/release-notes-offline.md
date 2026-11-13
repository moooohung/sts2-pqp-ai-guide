# pqp AI Guide (Offline) {{VERSION}}

**pqp AI Guide (Offline)** (mod ID `PqpAiGuideOffline`) is the standalone offline edition of the *Slay the Spire 2* AI guide mod. It is released exclusively here on GitHub Releases and is never published on the Steam Workshop.

## What's New in This Release (*Slay the Spire 2* v0.113.0)

- ***Slay the Spire 2* v0.113.0 support:** Updated for the v0.113.0 `public-beta` build, including original-content support for the new character **The Fool** (in **Play by Original Rules** across both editions, and in Standard-run recording in the shared `mod/` source / online edition).
- **AI guide & AI teammates temporarily paused until retrain:** Because v0.113.0 changes game rules, cards, enemies, and random generation, in-game AI recommendations and multiplayer AI teammates are paused until the AI model is retrained for v0.113.0. In the offline package, **Play by Original Rules** and the mod's Settings status line remain active while the bundled policy checkpoint stays inert.
- **All mod screens in 16 languages:** Every mod screen, settings row, and notice—including **My Replays** and **Local Storage** in the shared `mod/` source / online edition—is now localized across all 16 game languages.
- **Shared source (`mod/`) — new Local Storage screen:** Added a **Local Storage** view in the shared source / online edition to inspect or delete waiting and backup run recordings on this PC without affecting replays already uploaded to `sts.pqp.me` (not compiled into the offline binary, which never records runs).
- **Fixes:** Mod settings rows now install in Settings → General even while the AI guide is paused; in the shared source / online edition, a server-refused upload no longer raises the storage-full warning.

## What It Does

- **Play by Original Rules (`원본 규칙으로 플레이`):** Lets standard solo and multiplayer runs use original v0.113.0 content pools (including **The Fool**) and suspend game-logic patches from other mods without restarting. *(Active in this release.)*
- **In-game AI recommendations:** Shows pqp AI recommendation percentages (`%`) on cards, relics, potions, events, rest sites, shops, map paths, and combat choices. *(Paused on v0.113.0 until retraining completes.)*
- **Multiplayer AI teammates:** Lets the host add pqp AI teammates in multiplayer lobbies. *(Paused on v0.113.0 until retraining completes.)*
- **Bundled, fixed AI model:** Includes a fixed AI policy checkpoint in `PqpAiGuideOffline/policy/` verified by SHA-256 (kept inert in this release while the v0.113.0 guide is paused).

## What It Does Not Do

- **Never connects to the internet:** Contains no networking code and makes no network requests.
- **No recording or uploads:** Does not record runs, upload replays, or send training data.
- **No online UI or prompts:** Has no first-launch data-sharing consent popup, no in-game **My Replays** or **Local Storage** menu, no automatic model updates, and no external websites.

## Installation & Requirements

1. Download `{{ZIP}}` from this release and unzip it into `<game>\mods\` (your *Slay the Spire 2* folder's `mods` directory, creating `<game>\mods\PqpAiGuideOffline\`).
2. ***Slay the Spire 2* v0.113.0 (`public-beta`) only:** This release works only on the v0.113.0 Steam public beta build of *Slay the Spire 2*. On any other game build, the mod turns itself off automatically.
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

## 이번 릴리스 변경사항 (*Slay the Spire 2* v0.113.0)

- ***Slay the Spire 2* v0.113.0 지원:** v0.113.0 공개 베타(`public-beta`) 빌드에 맞춰 업데이트되었으며, 신규 캐릭터 **The Fool**을 포함한 원본 콘텐츠를 지원합니다(양쪽 에디션의 **‘원본 규칙으로 플레이’** 및 공용 `mod/` 소스·온라인 에디션의 일반 런 기록기 지원).
- **재학습 완료 전까지 AI 가이드·AI 동료 일시 중지:** v0.113.0에서 게임 규칙·카드·적·난수 생성이 변경됨에 따라, 새 버전에 맞춘 AI 모델 재학습이 끝날 때까지 인게임 AI 추천과 멀티플레이 AI 동료 기능은 일시 중지됩니다. 오프라인 패키지에서도 **‘원본 규칙으로 플레이’**와 설정 내 상태 표시 줄은 정상 작동하며, 동봉된 정책 체크포인트는 비활성 상태로 유지됩니다.
- **모든 모드 화면 16개 언어 지원:** 설정 항목과 안내 문구를 비롯해 공용 `mod/` 소스(온라인 에디션)의 **‘내 리플레이’** 창과 **‘로컬 저장 공간’** 화면까지 모든 모드 UI가 게임의 16개 언어를 지원합니다.
- **공용 소스(`mod/`) — ‘로컬 저장 공간’ 화면 추가:** 공용 소스·온라인 에디션에서 이 PC에 남아 있는 대기·백업 런 기록을 확인하거나 삭제할 수 있는 **‘로컬 저장 공간’** 화면이 추가되었습니다(`sts.pqp.me`에 이미 올라간 리플레이에는 영향 없음; 런 기록을 하지 않는 오프라인 빌드에는 포함되지 않습니다).
- **오류 수정:** AI 가이드가 일시 중지된 상태에서도 설정 → 일반 탭에 모드 설정 항목이 정상 표시되도록 수정했으며, 공용 소스·온라인 에디션에서 서버가 거부한 업로드 때문에 저장 공간 가득 참 경고가 뜨던 문제를 수정했습니다.

## 주요 기능

- **원본 규칙으로 플레이:** 일반 싱글플레이 및 멀티플레이 런에서 게임을 재시작하지 않고도 v0.113.0 원본 콘텐츠 풀(**The Fool** 포함)을 사용하고 다른 모드의 게임 로직 패치를 일시 중지할 수 있습니다. *(이번 릴리스에서 정상 작동)*
- **인게임 AI 추천 확률 표시:** 카드, 유물, 포션, 이벤트, 휴식 장소, 상점, 지도 경로 및 전투 선택지에 pqp AI 추천 확률(`%`)을 표시합니다. *(v0.113.0 재학습 완료 전까지 일시 중지)*
- **멀티플레이 AI 동료:** 멀티플레이 로비에서 호스트가 조종하는 AI 동료를 추가해 함께 플레이할 수 있습니다. *(v0.113.0 재학습 완료 전까지 일시 중지)*
- **고정 내장형 AI 모델:** `PqpAiGuideOffline/policy/` 폴더에 SHA-256 해시로 검증되는 고정 AI 모델 체크포인트가 포함됩니다(이번 v0.113.0 빌드에서는 가이드 일시 중지에 따라 비활성 상태로 유지됩니다).

## 포함되지 않는 기능 (완전 오프라인)

- **인터넷 연결 없음:** 네트워크에 전혀 연결하지 않습니다.
- **기록 및 업로드 없음:** 플레이 기록 저장, 리플레이 업로드, 학습 데이터 전송을 일절 수행하지 않습니다.
- **온라인 메뉴 및 동의 창 없음:** 첫 실행 데이터 제공 동의 창, 게임 내 **‘내 리플레이’** 및 **‘로컬 저장 공간’** 메뉴, 모델 자동 업데이트, 외부 웹사이트 연결 기능이 없습니다.

## 설치 방법 및 주의사항

1. 이 릴리스에서 `{{ZIP}}` 파일을 내려받아 `<game>\mods\` 폴더(*Slay the Spire 2*가 설치된 폴더 안의 `mods` 폴더)에 압축을 풉니다(`<game>\mods\PqpAiGuideOffline\` 폴더가 생성됩니다).
2. ***Slay the Spire 2* v0.113.0 공개 베타 빌드 전용:** v0.113.0 Steam 공개 베타(`public-beta`) 빌드에서만 작동하며, 다른 게임 빌드에서는 모드가 자동으로 꺼집니다.
3. **두 에디션 중 하나만 사용:** 온라인 에디션(**pqp AI Guide & Replay / 피큐피 AI 가이드 & 리플레이**)과 동시에 켜 두지 마세요. 온라인 에디션이 활성화되어 있으면 오프라인 에디션은 자동으로 대기 상태가 되어 아무 동작도 하지 않습니다.

## 라이선스 안내

- **소스 코드 (`AGPL-3.0-or-later`):** 모드의 소스 코드(https://github.com/moooohung/sts2-pqp-ai-guide)는 `AGPL-3.0-or-later` 라이선스(`LICENSE` 및 `LICENSE-EXCEPTION.md`)가 적용됩니다.
- **내장 AI 모델 가중치 (`PqpAiGuideOffline/policy/MODEL-LICENSE.md`):** 소스 코드의 AGPL-3.0 라이선스는 모델 가중치에 적용되지 않습니다. `PqpAiGuideOffline/policy/MODEL-LICENSE.md`에 따라 모델 가중치는 본 모드 내부에서만 사용할 수 있으며, 별도 추출, 수정, 재배포 및 타 모델 학습·증류에 사용하는 행위는 금지됩니다.

*본 프로젝트는 Mega Crit과 제휴 관계가 아닙니다.*

