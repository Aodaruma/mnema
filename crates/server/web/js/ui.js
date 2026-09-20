const view = document.querySelector("#view");
const title = document.querySelector("#view-title");
const eyebrow = document.querySelector("#view-eyebrow");
const connection = document.querySelector("#connection-chip");
const taskDialog = document.querySelector("#task-dialog");
const taskForm = document.querySelector("#task-form");
const habitDialog = document.querySelector("#habit-dialog");
const habitForm = document.querySelector("#habit-form");
const toastElement = document.querySelector("#toast");
const themeButton = document.querySelector("#theme-button");

const routeMeta = {
  home: ["TODAY", "今日を整える"],
  inbox: ["CAPTURE", "Inbox"],
  schedule: ["TIME MAP", "Schedule"],
  calendar: ["CONNECTED TIME", "Calendar"],
  habits: ["REPEAT", "Habits"],
  settings: ["PREFERENCES", "Settings"],
};

export function render(state) {
  const route = routeMeta[state.route] ? state.route : "home";
  const [overline, heading] = routeMeta[route];
  eyebrow.textContent = overline;
  title.textContent = heading;
  document.title = `${heading} · Mnema`;
  document.querySelectorAll("[data-route]").forEach((item) => {
    const active = item.dataset.route === route;
    item.classList.toggle("active", active);
    if (active) item.setAttribute("aria-current", "page");
    else item.removeAttribute("aria-current");
  });
  const darkTheme = state.settings.theme === "dark";
  themeButton.setAttribute("aria-pressed", String(darkTheme));
  renderConnection(state.health);
  view.setAttribute("aria-busy", String(Boolean(state.loading)));

  if (state.loading && !state.health) {
    view.innerHTML = loadingState("Mnema を準備しています");
    return;
  }

  const renderers = {
    inbox: renderInbox,
    schedule: renderSchedule,
    calendar: renderCalendar,
    habits: renderHabits,
    settings: renderSettings,
    home: renderHome,
  };
  view.innerHTML = `${renderRefreshError(state.error)}${renderers[route](state)}`;
}

function renderConnection(health) {
  connection.classList.remove("online", "offline");
  if (health?.status === "ok") {
    connection.classList.add("online");
    connection.querySelector("span:last-child").textContent = `${health.backend} · online`;
  } else if (health?.status === "offline") {
    connection.classList.add("offline");
    connection.querySelector("span:last-child").textContent = "offline";
  } else {
    connection.querySelector("span:last-child").textContent = "接続確認中";
  }
}

function renderHome(state) {
  const active = state.tasks.filter((task) => !task.completed);
  const dueToday = active.filter((task) => task.due_date && task.due_date <= state.selectedDate);
  const hasPendingPreview = state.preview?.date === state.selectedDate && !state.preview.applied;
  const agenda = hasPendingPreview ? state.preview.blocks : state.schedule;
  const plannedMinutes = agenda.reduce((sum, block) => sum + blockMinutes(block), 0);
  const day = dateParts(state.selectedDate);
  const timezone = state.preferences?.timezone || state.settings.timezone;
  return `
    <div class="view-stack">
      <section class="card hero-card">
        <div>
          <p class="eyebrow">A QUIET PLAN FOR THE DAY</p>
          <h2>${greeting()}。<br />頭の外に置いて、ひとつずつ。</h2>
          <p>Calendar・Habit・生活時間を守りながら、空いている時間へタスクを並べます。適用前に差分を確認できます。</p>
        </div>
        <div class="hero-date"><strong>${day.day}</strong><span>${day.month} · ${day.weekday}</span></div>
      </section>
      <section class="metrics" aria-label="今日の概要">
        <div class="metric"><span>Open tasks</span><strong>${active.length}</strong></div>
        <div class="metric"><span>Due now</span><strong>${dueToday.length}</strong></div>
        <div class="metric"><span>Planned</span><strong>${formatDuration(plannedMinutes)}</strong></div>
      </section>
      <section class="grid grid-main">
        <article class="card">
          <div class="card-head"><div><h2>今日の Agenda</h2><p>${hasPendingPreview ? "未適用のプレビュー" : "保存済みの予定"}</p></div><a class="button button-quiet button-small" href="#schedule">7日プラン</a></div>
          <div class="plan-toolbar">
            <label class="field date-field"><span>対象日</span><input type="date" value="${escapeAttr(state.selectedDate)}" data-action="select-date" /></label>
            <label class="field"><span>開始</span><input type="time" value="${escapeAttr(state.settings.planningStart)}" data-plan="start" /></label>
            <label class="field"><span>終了</span><input type="time" value="${escapeAttr(state.settings.planningEnd)}" data-plan="end" /></label>
            <div class="plan-actions"><button class="button button-secondary" type="button" data-action="preview-plan">Preview</button><button class="button button-primary" type="button" data-action="apply-plan">Apply</button></div>
          </div>
          ${renderIssues(state.preview?.issues)}
          <div class="section-gap"></div>
          ${renderAgenda(agenda, hasPendingPreview ? "preview" : "saved", timezone)}
        </article>
        <aside class="card"><div class="card-head"><div><h2>次のタスク</h2><p>期限と見積をもとに表示</p></div><a class="button button-quiet button-small" href="#inbox">すべて見る</a></div>${renderTaskList(active.slice(0, 7))}</aside>
      </section>
    </div>`;
}

function renderInbox(state) {
  const active = state.tasks.filter((task) => !task.completed);
  const completed = state.tasks.filter((task) => task.completed);
  return `<div class="view-stack">
    <section class="section-tools"><div class="section-title"><h2>頭の中から、まずここへ。</h2><p>日付と見積は後からでも変更できます。</p></div><button class="button button-primary" type="button" data-action="new-task">＋ タスクを追加</button></section>
    <section class="grid grid-equal"><article class="card"><div class="card-head"><div><h2>Open</h2><p>${active.length} tasks</p></div></div>${renderTaskList(active)}</article><article class="card card-subtle"><div class="card-head"><div><h2>Completed</h2><p>${completed.length} tasks</p></div></div>${renderTaskList(completed)}</article></section>
  </div>`;
}

function renderSchedule(state) {
  const timezone = state.preferences?.timezone || state.settings.timezone;
  return `<div class="view-stack">
    <section class="section-tools"><div class="section-title"><h2>時間の地図</h2><p>今日の確定済み block と、7日間の変更案を分けて確認します。</p></div><label class="field"><span>表示日</span><input type="date" value="${escapeAttr(state.selectedDate)}" data-action="select-date" /></label></section>
    <section class="grid grid-main">
      <article class="card"><div class="card-head"><div><h2>${formatJapaneseDate(state.selectedDate)}</h2><p>保存済み schedule</p></div><span class="state-pill">${state.schedule.length} blocks</span></div>${renderAgenda(state.schedule, "saved", timezone)}</article>
      <aside class="card"><div class="card-head"><div><h2>7日 Auto Schedule</h2><p>Calendar・Habit・Sleep・Travel を制約にします</p></div></div>
        <div class="stack-actions"><button class="button button-secondary" type="button" data-action="preview-auto">差分を Preview</button><button class="button button-primary" type="button" data-action="apply-auto" ${state.autoPreview?.changed_count > 0 ? "" : "disabled"}>Fingerprint で Apply</button></div>
        <p class="settings-note">Apply は直前 preview の fingerprint が一致した場合だけ成功します。予定が変わった場合は 409 となり、再 Preview が必要です。</p>
      </aside>
    </section>
    <section class="card"><div class="card-head"><div><h2>変更差分</h2><p>${state.autoPreview ? `${state.autoPreview.changed_count} changes · ${state.autoPreview.start_date}〜${state.autoPreview.end_date_exclusive}` : "まだ preview していません"}</p></div>${state.autoPreview ? `<span class="state-pill">${escapeHtml(state.autoPreview.fingerprint.slice(0, 10))}…</span>` : ""}</div>${renderAutoDiff(state.autoPreview, timezone)}</section>
  </div>`;
}

function renderCalendar(state) {
  const configured = state.health?.integrations?.google_calendar_configured;
  return `<div class="view-stack">
    <section class="section-tools"><div class="section-title"><h2>Calendar connection</h2><p>読み込む Calendar と、Mnema が書き戻す専用 Calendar を分離します。</p></div><div class="stack-actions horizontal"><label class="compact-field"><span>接続権限</span><select class="compact-select" data-calendar-access><option value="read_write">読み書き</option><option value="read_only">読み取りのみ</option></select></label><button class="button button-primary" type="button" data-action="connect-calendar" ${configured ? "" : "disabled"}>Google を接続</button></div></section>
    ${configured ? "" : `<section class="card warning-card"><strong>Google Calendar は未設定です</strong><p><code>MNEMA_GOOGLE_CLIENT_ID</code> と <code>MNEMA_GOOGLE_REDIRECT_URI</code> を設定して再起動してください。Calendar API は明示的に 503 を返します。</p></section>`}
    ${state.calendarAccounts.length ? state.calendarAccounts.map((account) => renderCalendarAccount(state, account)).join("") : `<section class="card">${emptyState("◫", "接続済み Calendar はありません", "OAuth 接続後、取り込む Calendar を選択してください。")}</section>`}
  </div>`;
}

function renderCalendarAccount(state, account) {
  const calendars = state.remoteCalendars[account.id];
  const selectedIds = new Set(account.selected_calendar_ids || []);
  const canWrite = account.access_mode === "READ_WRITE";
  return `<section class="card calendar-account">
    <div class="card-head"><div><h2>${escapeHtml(account.display_name)}</h2><p>${escapeHtml(account.email || account.provider_account_id)} · ${escapeHtml(account.access_mode)}</p></div><span class="state-pill">${account.enabled ? "connected" : "disabled"}</span></div>
    <div class="account-meta"><span>Timezone: ${escapeHtml(account.timezone || "not reported")}</span><span>Managed: ${escapeHtml(account.managed_calendar_id || "not created")}</span></div>
    <div class="stack-actions horizontal wrap"><button class="button button-secondary button-small" type="button" data-action="load-calendars" data-id="${escapeAttr(account.id)}">Calendar を選ぶ</button><button class="button button-secondary button-small" type="button" data-action="sync-calendar" data-id="${escapeAttr(account.id)}">今すぐ同期</button><button class="button button-secondary button-small" type="button" data-action="ensure-managed" data-id="${escapeAttr(account.id)}" ${canWrite ? "" : "disabled"}>Managed Calendar</button><button class="button button-primary button-small" type="button" data-action="writeback-calendar" data-id="${escapeAttr(account.id)}" ${canWrite && account.managed_calendar_id ? "" : "disabled"}>予定を書き戻す</button></div>
    ${calendars ? renderCalendarPicker(account, calendars, selectedIds) : ""}
  </section>`;
}

function renderCalendarPicker(account, calendars, selectedIds) {
  if (!calendars.length) {
    return `<div class="calendar-picker">${emptyState("◫", "選択できる Calendar はありません", "Google Calendar 側の公開範囲と権限を確認してください。")}</div>`;
  }
  return `<div class="calendar-picker">${calendars.map((calendar) => `<label><input type="checkbox" data-calendar-choice="${escapeAttr(account.id)}" value="${escapeAttr(calendar.id)}" ${selectedIds.has(calendar.id) ? "checked" : ""} /><span><strong>${escapeHtml(calendar.summary)}</strong><small>${calendar.primary ? "primary · " : ""}${escapeHtml(calendar.access_role || "")}</small></span></label>`).join("")}<button class="button button-primary button-small" type="button" data-action="save-calendar-selection" data-id="${escapeAttr(account.id)}">選択を保存</button></div>`;
}

function renderHabits(state) {
  return `<div class="view-stack">
    <section class="section-tools"><div class="section-title"><h2>繰り返しを、無理なく。</h2><p>Habit は日ごとの occurrence に展開され、空き時間へ自動配置されます。</p></div><div class="stack-actions horizontal"><button class="button button-secondary" type="button" data-action="expand-habits">7日分を展開</button><button class="button button-primary" type="button" data-action="new-habit">＋ Habit を追加</button></div></section>
    <section class="grid grid-main"><article class="card"><div class="card-head"><div><h2>Active habits</h2><p>${state.habits.length} rules</p></div></div>${renderHabitList(state.habits)}</article><aside class="card"><div class="card-head"><div><h2>Occurrences</h2><p>${state.occurrences.length ? "展開済みの7日間" : "展開ボタンで生成"}</p></div></div>${renderOccurrences(state)}</aside></section>
  </div>`;
}

function renderHabitList(habits) {
  if (!habits.length) return emptyState("↻", "Habit はまだありません", "短い習慣から追加してみましょう。");
  return `<div class="habit-list">${habits.map((habit) => `<div class="habit-row"><div><strong>${escapeHtml(habit.title)}</strong><p>${escapeHtml(habitScheduleLabel(habit))} · ${habit.duration_minutes}分 · ${habit.flexibility === "REQUIRED" ? "必須" : "柔軟"}${habit.preferred_window ? ` · ${escapeHtml(habit.preferred_window.start)}–${escapeHtml(habit.preferred_window.end)}` : ""}</p></div><div class="stack-actions horizontal"><button class="mini-action" type="button" data-action="edit-habit" data-id="${escapeAttr(habit.id)}" aria-label="${escapeAttr(`${habit.title}を編集`)}" title="編集">✎</button><button class="mini-action danger" type="button" data-action="disable-habit" data-id="${escapeAttr(habit.id)}" aria-label="${escapeAttr(`${habit.title}を無効化`)}" title="無効化">×</button></div></div>`).join("")}</div>`;
}

function renderOccurrences(state) {
  if (!state.occurrences.length) return emptyState("○", "Occurrence は未展開です", "7日分を展開すると、skip / snooze を操作できます。");
  const habitNames = new Map(state.habits.map((habit) => [habit.id, habit.title]));
  return `<div class="occurrence-list">${state.occurrences.map((item) => `<div class="occurrence-row"><div><strong>${escapeHtml(habitNames.get(item.habit_id) || "Habit")}</strong><p>${escapeHtml(item.occurrence_date)} · ${escapeHtml(item.state)}</p></div><div class="stack-actions horizontal"><button class="button button-quiet button-small" type="button" data-action="skip-occurrence" data-id="${escapeAttr(item.id)}" ${["DONE", "SKIPPED"].includes(item.state) ? "disabled" : ""}>Skip</button><button class="button button-quiet button-small" type="button" data-action="snooze-occurrence" data-id="${escapeAttr(item.id)}" ${["DONE", "SKIPPED"].includes(item.state) ? "disabled" : ""}>Snooze</button></div></div>`).join("")}</div>`;
}

function renderSettings(state) {
  const preferences = state.preferences;
  const work = firstRange(preferences?.named_hours?.find((policy) => policy.name.toLowerCase() === "work")) || { start: state.settings.planningStart, end: state.settings.planningEnd };
  const sleep = firstRange(preferences?.sleep) || { start: "23:00", end: "07:00" };
  return `<form id="settings-form" class="view-stack">
    <section class="settings-grid">
      <article class="settings-section"><h2>Hours</h2><p>平日の自動配置可能時間です。</p><label class="field"><span>IANA timezone</span><input name="timezone" value="${escapeAttr(preferences?.timezone || state.settings.timezone)}" placeholder="Asia/Tokyo" required /></label><div class="field-row"><label class="field"><span>Work start</span><input name="workStart" type="time" value="${escapeAttr(work.start)}" required /></label><label class="field"><span>Work end</span><input name="workEnd" type="time" value="${escapeAttr(work.end)}" required /></label></div></article>
      <article class="settings-section"><h2>Sleep</h2><p>毎日、必ず守る hard constraint です。日をまたぐ時間にも対応します。</p><div class="field-row"><label class="field"><span>Sleep start</span><input name="sleepStart" type="time" value="${escapeAttr(sleep.start)}" required /></label><label class="field"><span>Sleep end</span><input name="sleepEnd" type="time" value="${escapeAttr(sleep.end)}" required /></label></div></article>
      <article class="settings-section"><h2>Travel</h2><p>場所付き外部予定の前後へ確保する既定 buffer です。</p><label class="field"><span>Travel buffer（分）</span><input name="travelBuffer" type="number" min="0" max="360" value="${preferences?.default_travel_buffer_minutes ?? 15}" /></label><p class="settings-note">個別 Calendar event に override があれば、そちらを優先します。</p></article>
      <article class="settings-section"><h2>Appearance & refresh</h2><p>このブラウザだけに保存する表示設定です。</p><label class="field"><span>Theme</span><select name="theme">${option("light", "Light", state.settings.theme)}${option("dark", "Dark", state.settings.theme)}</select></label><label class="field"><span>Auto refresh（秒）</span><input name="refreshSeconds" type="number" min="5" max="3600" value="${state.settings.refreshSeconds}" /></label><p class="settings-note">Background apply は server 側で <code>MNEMA_AUTOMATION_MODE=auto_silent</code> を明示した場合だけ有効です。</p></article>
    </section>
    <div class="dialog-actions"><button class="button button-primary" type="submit">設定を保存</button></div>
  </form>`;
}

function renderTaskList(tasks) {
  if (!tasks.length) return emptyState("✓", "ここは空です", "追加したタスクがここに表示されます。");
  return `<div class="task-list">${tasks.map((task) => `<div class="task-row ${task.completed ? "completed" : ""}"><input class="task-check" type="checkbox" data-action="toggle-task" data-id="${escapeAttr(task.id)}" ${task.completed ? "checked" : ""} aria-label="${escapeAttr(`${task.title}の完了状態`)}" /><div class="task-copy"><p class="task-title">${escapeHtml(task.title)}</p><div class="task-meta">${task.due_date ? `<span>期限 ${formatShortDate(task.due_date)}</span>` : ""}${task.estimated_minutes ? `<span>${task.estimated_minutes}分</span>` : ""}</div></div><div class="task-actions"><button class="mini-action" type="button" data-action="edit-task" data-id="${escapeAttr(task.id)}" aria-label="${escapeAttr(`${task.title}を編集`)}" title="編集">✎</button><button class="mini-action danger" type="button" data-action="delete-task" data-id="${escapeAttr(task.id)}" aria-label="${escapeAttr(`${task.title}を削除`)}" title="削除">×</button></div></div>`).join("")}</div>`;
}

function renderAgenda(blocks, kind, timezone) {
  if (!blocks?.length) return emptyState("◷", "予定はまだありません", "Preview で空き時間へタスクを配置できます。");
  return `<div class="agenda">${blocks.map((block) => `<div class="agenda-row ${kind === "preview" ? "preview" : ""}"><time class="agenda-time" datetime="${escapeAttr(block.start_at)}">${timeFromIso(block.start_at, timezone)}</time><span class="agenda-line" aria-hidden="true"></span><div class="agenda-block"><strong>${escapeHtml(block.title)}</strong><span>${timeFromIso(block.start_at, timezone)}–${timeFromIso(block.end_at, timezone)} · ${escapeHtml(block.block_type || "task")}</span></div></div>`).join("")}</div>`;
}

function renderAutoDiff(preview, timezone) {
  if (!preview) return emptyState("⇄", "差分はまだありません", "Preview は保存を変更せず、作成・移動・削除を一覧化します。");
  const changes = (preview.changes || []).filter((change) => change.kind !== "UNCHANGED");
  const issueText = (preview.issues || []).map((issue) => `<li>${escapeHtml(issueLabel(issue))}</li>`).join("");
  return `${issueText ? `<ul class="issue-list">${issueText}</ul>` : ""}<div class="diff-list">${changes.length ? changes.map((change) => `<div class="diff-row"><span class="change-kind ${escapeAttr(change.kind.toLowerCase())}">${escapeHtml(change.kind)}</span><div><strong>${escapeHtml(change.title)}</strong><p>${escapeHtml(change.reason)}</p><small>${diffTime(change.before, timezone)} → ${diffTime(change.after, timezone)}</small></div></div>`).join("") : emptyState("✓", "変更はありません", "現在のプランと一致しています。")}</div>`;
}

function renderIssues(issues) {
  if (!issues?.length) return "";
  return `<ul class="issue-list">${issues.map((issue) => `<li>${escapeHtml(issue.message)}</li>`).join("")}</ul>`;
}

function renderRefreshError(error) {
  if (!error) return "";
  return `<section class="error-banner" role="alert"><div><strong>最新データを取得できませんでした</strong><p>${escapeHtml(error)}</p></div><button class="button button-secondary button-small" type="button" data-action="retry-refresh">再試行</button></section>`;
}

function loadingState(message) {
  return `<div class="loading-state" role="status"><span class="spinner" aria-hidden="true"></span><span>${escapeHtml(message)}</span></div>`;
}

function emptyState(icon, heading, copy) {
  return `<div class="empty-state"><div><span class="empty-state-icon">${icon}</span><strong>${heading}</strong><span>${copy}</span></div></div>`;
}

export function openTaskDialog(task = null) {
  taskForm.reset();
  taskForm.elements.id.value = task?.id || "";
  taskForm.elements.title.value = task?.title || "";
  taskForm.elements.description.value = task?.description || "";
  taskForm.elements.due_date.value = task?.due_date || "";
  taskForm.elements.estimated_minutes.value = task?.estimated_minutes || "";
  taskForm.elements.completed.checked = Boolean(task?.completed);
  taskForm.querySelector(".edit-only").hidden = !task;
  document.querySelector("#task-dialog-title").textContent = task ? "タスクを編集" : "タスクを追加";
  taskDialog.showModal();
  taskForm.elements.title.focus();
}

export function closeTaskDialog() { taskDialog.close(); }
export function getTaskForm() { return taskForm; }

export function openHabitDialog(habit = null) {
  habitForm.reset();
  habitForm.elements.id.value = habit?.id || "";
  habitForm.elements.title.value = habit?.title || "";
  habitForm.elements.scheduleKind.value = habit?.schedule?.kind || "DAILY";
  habitForm.elements.durationMinutes.value = habit?.duration_minutes || 30;
  habitForm.elements.preferredStart.value = habit?.preferred_window?.start || "";
  habitForm.elements.preferredEnd.value = habit?.preferred_window?.end || "";
  habitForm.elements.flexibility.value = habit?.flexibility || "FLEXIBLE";
  const selectedDays = new Set(habit?.schedule?.weekdays || ["MONDAY", "TUESDAY", "WEDNESDAY", "THURSDAY", "FRIDAY"]);
  habitForm.querySelectorAll("[name='weekdays']").forEach((box) => { box.checked = selectedDays.has(box.value); });
  syncHabitWeekdayAvailability();
  document.querySelector("#habit-dialog-title").textContent = habit ? "Habit を編集" : "Habit を追加";
  habitDialog.showModal();
  habitForm.elements.title.focus();
}

export function closeHabitDialog() { habitDialog.close(); }
export function getHabitForm() { return habitForm; }

export function syncHabitWeekdayAvailability() {
  const picker = habitForm.querySelector("[data-weekday-picker]");
  picker.disabled = habitForm.elements.scheduleKind.value !== "WEEKDAYS";
}

export function showToast(message, type = "success") {
  toastElement.setAttribute("role", type === "error" ? "alert" : "status");
  toastElement.setAttribute("aria-live", type === "error" ? "assertive" : "polite");
  toastElement.textContent = message;
  toastElement.className = `toast visible ${type}`;
  clearTimeout(showToast.timer);
  showToast.timer = setTimeout(() => { toastElement.className = "toast"; }, 3600);
}

export function setRefreshBusy(busy) {
  const button = document.querySelector("#refresh-button");
  button.disabled = busy;
  button.setAttribute("aria-busy", String(busy));
  button.setAttribute("aria-label", busy ? "更新中" : "更新");
  button.classList.toggle("is-spinning", busy);
}

function greeting() {
  const hour = new Date().getHours();
  if (hour < 11) return "おはようございます";
  if (hour < 18) return "こんにちは";
  return "こんばんは";
}

function blockMinutes(block) { return Math.max(0, Math.round((new Date(block.end_at) - new Date(block.start_at)) / 60000)); }
function formatDuration(minutes) { return minutes >= 60 ? `${Math.floor(minutes / 60)}h ${minutes % 60 ? `${minutes % 60}m` : ""}` : `${minutes}m`; }
function timeFromIso(value, timezone) {
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) return "--:--";
  try {
    return date.toLocaleTimeString("ja-JP", { hour: "2-digit", minute: "2-digit", timeZone: timezone });
  } catch (_) {
    return date.toLocaleTimeString("ja-JP", { hour: "2-digit", minute: "2-digit" });
  }
}
function dateParts(value) { const date = new Date(`${value}T12:00:00`); return { day: String(date.getDate()).padStart(2, "0"), month: date.toLocaleDateString("en-US", { month: "short" }), weekday: date.toLocaleDateString("en-US", { weekday: "short" }) }; }
function formatJapaneseDate(value) { return new Date(`${value}T12:00:00`).toLocaleDateString("ja-JP", { year: "numeric", month: "long", day: "numeric", weekday: "short" }); }
function formatShortDate(value) { return new Date(`${value}T12:00:00`).toLocaleDateString("ja-JP", { month: "short", day: "numeric" }); }
function option(value, label, current) { return `<option value="${value}" ${value === current ? "selected" : ""}>${label}</option>`; }
function firstRange(policy) { return policy?.days?.find((day) => day.ranges?.length)?.ranges?.[0] || null; }
function habitScheduleLabel(habit) { return habit.schedule.kind === "DAILY" ? "毎日" : habit.schedule.weekdays.map((day) => day.slice(0, 3)).join("・"); }
function diffTime(snapshot, timezone) {
  if (!snapshot) return "なし";
  const date = new Date(snapshot.start_at);
  if (Number.isNaN(date.valueOf())) return "日時不明";
  try {
    return date.toLocaleString("ja-JP", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", timeZone: timezone });
  } catch (_) {
    return date.toLocaleString("ja-JP", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  }
}
function issueLabel(issue) { if (typeof issue === "string") return issue; if (issue?.ItemUnscheduled) return `${issue.ItemUnscheduled.title}: ${issue.ItemUnscheduled.reason}`; return JSON.stringify(issue); }
function escapeHtml(value) { return String(value ?? "").replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;"); }
function escapeAttr(value) { return escapeHtml(value); }
