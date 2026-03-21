# OpenApe CLI (Unofficial)

中原大學 [i-Learning](https://ilearning.cycu.edu.tw/) (Moodle) 平台自動化命令列工具 (CLI)，幫助你快速查詢課程、觀看影片、下載教材。

## 功能特色
- 📚 **課程資訊**：列出修課清單、成績、課程大綱與進度。
- 📺 **影片輔助**：列出/下載影片，甚至自動標記為已觀看。
- 📝 **測驗與教材**：快速查看測驗、下載教材。
- 💬 **討論區與公告**：在終端機直接閱讀公告與討論區。
- 📅 **行事曆**：內建行事曆事件查詢與匯出。
- 🤖 **Agent Skills**：提供 Claude Code 等 Skills 工作流支援。

## 安裝

透過 npm 安裝（[Node.js v18+](https://nodejs.org/)）：
```bash
npm install -g openape
```

## 核心指令

### 登入與驗證 (Authentication)
第一次使用需要登入，會開啟瀏覽器讓你手動完成登入，隨後會快取 Session 供未來使用。
```bash
openape login          # 登入並儲存 session (開啟瀏覽器)
openape auth status    # 檢查當前登入狀態
openape auth logout    # 登出並清除 session
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
openape videos complete-all <id>     # 影片批次完成
openape videos download <id>         # 下載影片
```

### 測驗與教材 (Quizzes & Materials)
```bash
openape quizzes list <course-id>     # 列出特定課程測驗
openape quizzes list-all             # 列出所有課程測驗
openape quizzes open <id>            # 開啟特定測驗
openape materials list-all           # 列出所有可下載教材
openape materials download <id>      # 下載指定教材
openape materials download-all       # 批次下載教材
```

### 成績與其他查詢 (Grades, Forums, Calendar)
```bash
openape grades summary               # 顯示學期成績總覽
openape grades course <id>           # 顯示特定課程成績
openape forums list <course-id>      # 列出課程論壇
openape announcements list-all       # 列出所有公告
openape announcements read <id>      # 閱讀特定公告
openape calendar events              # 查詢行事曆事件
openape calendar export              # 匯出事件
```

### Skills
讓你的 AI Agent 也可以控制 OpenApe。只需一個指令即可安裝：
```bash
openape skills list                  # 查看目前提供的所有 skills
openape skills install claude        # 為 Claude Code 安裝技能 (支援 claude, codex, opencode)
openape skills install --all         # 自動偵測環境並安裝給所有支援的 Agent
```
也可以透過 `npx skills` 安裝：
```bash
npx skills add openape/openape
```

## 開發

專案使用 [Deno](https://deno.land/) 開發，歡迎一同貢獻：

```bash
git clone https://github.com/mo7yw4ng/openape && cd openape

# 啟動開發伺服器 (將直接執行 src/index.ts)
deno task dev

# 編譯成各平台執行檔 (預設輸出到 dist/OpenApe.exe)
deno task compile
```

## 版權與授權

此專案之版權規範採用 **MIT License** - 至 [LICENSE](LICENSE) 查看更多相關聲明

> **免責聲明**：本工具為非官方開放原始碼專案，與中原大學官方無關。請斟酌使用腳本輔助功能，避免不當操作（如短時間發送大量請求修改系統狀態）而違反學術倫理或導致帳號遭封鎖。