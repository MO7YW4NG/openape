<h1 align="center">
  <img src="assets/logo.svg" width="150" /><br/>
  OpenApe CLI (Unofficial)<br/>
  <a href="https://www.npmjs.com/package/@mo7yw4ng/openape"><img src="https://img.shields.io/npm/v/@mo7yw4ng/openape" alt="npm version" /></a>
  <a href="https://www.npmjs.com/package/@mo7yw4ng/openape"><img src="https://img.shields.io/npm/dm/@mo7yw4ng/openape" alt="npm downloads" /></a>
</h1>

中原大學 [i-Learning](https://ilearning.cycu.edu.tw/) (Moodle) 平台自動化命令列工具 (CLI)，幫助你快速查詢課程、觀看影片、下載教材。

## 功能特色
- 📚 **課程資訊**：列出修課清單、成績、課程大綱與進度。
- 📺 **影片輔助**：列出/下載影片，甚至自動標記為已觀看。
- 📝 **測驗與教材**：快速進行測驗、下載教材。
- 💬 **討論區與公告**：閱讀公告、討論區、發表回覆。
- 📅 **行事曆**：內建行事曆事件查詢與匯出。
- ✅ **作業繳交**：查詢作業、檢查繳交狀態、上傳檔案並繳交。
- 🤖 **Agent Skills**：提供 Claude Code 等 Skills 工作流支援。

## 安裝

透過 npm 安裝為全域指令（推薦，[Node.js](https://nodejs.org/) v18+）：
```bash
npm install -g @mo7yw4ng/openape
```

或用 npx 單次執行（不需安裝）：
```bash
npx @mo7yw4ng/openape --help
```

## 核心指令

### 登入與驗證 (Authentication)
第一次使用需要登入，會開啟瀏覽器讓你手動完成登入，隨後會快取 Session 供未來使用。
```bash
openape login          # 登入並儲存 session (開啟瀏覽器)
openape status         # 檢查當前登入狀態
openape logout         # 登出並清除 session
```

### 課程 (Courses)
```bash
openape courses list               # 列出所有課程 (支援 --incomplete-only, --level)
openape courses info <id>          # 顯示特定課程的詳細資訊
openape courses progress <id>      # 顯示特定課程的進度
openape courses syllabus <id>      # 顯示課程大綱
```

### 影片 (Videos)
```bash
openape videos list <course-id>      # 列出課程中的影片
openape videos complete <id>         # 標記特定影片為已觀看
openape videos complete-all            # 影片批次完成
openape videos download <id>         # 下載影片 (支援 --output-dir)
```

### 測驗與教材 (Quizzes & Materials)
```bash
openape quizzes list <course-id>     # 列出特定課程測驗
openape quizzes list-all             # 列出所有課程測驗
openape quizzes start <quiz-id>      # 開始測驗
openape quizzes info <attempt-id>    # 查看測驗題目
openape quizzes save <attempt-id> '<answers>' # 儲存測驗答案
openape quizzes submit <attempt-id>           # 送出目前已儲存的測驗答案
openape materials list <course-id>    # 列出指定課程教材
openape materials list-all           # 列出所有可下載教材
openape materials download <id>      # 下載指定教材 (支援 --output-dir)
openape materials download-all       # 批次下載教材 (支援 --output-dir, --level)
openape materials complete <id>      # 標記教材為已完成
openape materials complete-all       # 批次標記教材為已完成
```

### 成績與其他查詢 (Grades, Forums, Calendar)
```bash
openape grades summary               # 顯示學期成績總覽
openape grades course <id>           # 顯示特定課程成績
openape forums list                  # 列出進行中課程的討論區
openape forums list-all              # 列出所有討論區
openape forums discussions <forum-id>      # 列出討論區中的討論串
openape forums posts <discussion-id>       # 列出討論串中的貼文
openape forums reply <post-id> <subject> <message>  # 回覆貼文 (支援 --attachment-id)
openape forums post <forum-id> <subject> <message>  # 發起新討論
openape forums delete <post-id>      # 刪除討論貼文
openape announcements list-all       # 列出所有公告
openape announcements read <id>      # 閱讀特定公告
openape calendar events              # 查詢行事曆事件 (支援 --upcoming, --days, --course)
openape calendar export              # 匯出事件 (支援 --output, --days)
```

### 作業與檔案上傳 (Assignments & Upload)
```bash
# 作業查詢與繳交
openape assignments list <course-id>       # 列出課程作業
openape assignments list-all               # 列出所有作業 (支援 --level all)
openape assignments status <assignment-id> # 檢查作業繳交狀態
openape assignments submit <assignment-id> # 繳交作業
  --text "內容"                            # 線上文字繳交
  --file-id <draft-id>                     # 使用已上傳的檔案 ID
  --file <path>                            # 直接上傳檔案並繳交

# 檔案上傳至草稿區
openape upload file <path>                 # 上傳檔案取得 draft ID
```

### 頁面 (Pages)
```bash
openape pages list <course-id>     # 列出課程頁面 (內容預覽前 150 字)
openape pages list-all             # 列出所有課程頁面 (支援 --level)
openape pages show <cmid>          # 顯示頁面完整內容
```

### Skills
讓你的 AI Agent 也可以控制 OpenApe。只需一個指令即可安裝：
```bash
openape skills install claude        # 為 Claude Code 安裝技能 (支援 claude, codex, opencode)
openape skills install --all         # 自動偵測環境並安裝給所有支援的 Agent
```
也可以透過 `npx skills` 安裝：
```bash
npx skills add mo7yw4ng/openape
```

## 開發

```bash
git clone https://github.com/mo7yw4ng/openape && cd openape

# 建置
cargo build

# 執行
cargo run -- --help
```

## 版權與授權

此專案之版權規範採用 **MIT License** - 至 [LICENSE](LICENSE) 查看更多相關聲明

> **免責聲明**：本工具為非官方開放原始碼專案，與中原大學官方無關。請斟酌使用腳本輔助功能，避免不當操作（如短時間發送大量請求修改系統狀態）而違反學術倫理或導致帳號遭封鎖。
