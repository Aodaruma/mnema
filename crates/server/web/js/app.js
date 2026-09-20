import { api, featureRequest } from "./api.js?v=0.0.1-s3";
import { applyServerDefaults, getState, saveSettings, subscribe, updateState } from "./store.js?v=0.0.1-s3";
import {
  closeHabitDialog,
  closeTaskDialog,
  getHabitForm,
  getTaskForm,
  openHabitDialog,
  openTaskDialog,
  render,
  setRefreshBusy,
  showToast,
  syncHabitWeekdayAvailability,
} from "./ui.js?v=0.0.1-s3";
import { calendarFeature, createCalendarClient } from "./features/calendar.js?v=0.0.1-s3";
import { createHabitsClient, habitsFeature } from "./features/habits.js?v=0.0.1-s3";

const calendar = createCalendarClient(featureRequest);
const habits = createHabitsClient(featureRequest);
window.mnemaFeatures = Object.freeze({ calendar: calendarFeature, habits: habitsFeature });

let refreshTimer = null;
let refreshing = false;

subscribe(render);
render(getState());

async function initialize() {
  updateState({ loading: true, error: null });
  await refreshData({ quiet: true });
  configureAutoRefresh();
}

async function refreshData({ quiet = false } = {}) {
  if (refreshing) return;
  refreshing = true;
  if (!quiet) setRefreshBusy(true);
  const state = getState();
  let health = null;
  try {
    health = await api.health();
    applyServerDefaults(health.defaults);
    const [taskResponse, scheduleResponse, preferences, habitResponse, accountResponse] = await Promise.all([
      api.tasks(),
      api.schedule(state.selectedDate),
      api.preferences(),
      habits.list(),
      calendar.accounts(),
    ]);
    updateState({
      health,
      tasks: taskResponse.tasks,
      schedule: scheduleResponse.blocks,
      preferences,
      habits: habitResponse.habits,
      calendarAccounts: accountResponse.accounts,
      loading: false,
      error: null,
    });
  } catch (error) {
    updateState({
      health: health || { status: "offline" },
      loading: false,
      error: error.message || "Server に接続できません。",
    });
    if (!quiet) showToast(error.message || "更新できませんでした。", "error");
  } finally {
    refreshing = false;
    setRefreshBusy(false);
  }
}

async function refreshSchedule() {
  try {
    const response = await api.schedule(getState().selectedDate);
    updateState({ schedule: response.blocks, preview: null, autoPreview: null });
  } catch (error) {
    showToast(error.message, "error");
  }
}

function configureAutoRefresh() {
  clearInterval(refreshTimer);
  const seconds = Math.max(5, Number(getState().settings.refreshSeconds) || 30);
  refreshTimer = setInterval(() => {
    if (document.visibilityState === "visible") refreshData({ quiet: true });
  }, seconds * 1000);
}

function planInput() {
  const state = getState();
  const timezone = state.preferences?.timezone || state.settings.timezone;
  return {
    date: state.selectedDate,
    availability_start: document.querySelector("[data-plan='start']")?.value || state.settings.planningStart,
    availability_end: document.querySelector("[data-plan='end']")?.value || state.settings.planningEnd,
    timezone_offset: timezoneOffsetForDate(timezone, state.selectedDate, state.settings.timezoneOffset),
  };
}

async function runPlan(apply) {
  const action = apply ? api.applyToday : api.previewToday;
  try {
    const response = await action(planInput());
    updateState({ preview: response, schedule: response.schedule });
    showToast(apply ? `${response.blocks.length} 件の block を保存しました。` : `${response.blocks.length} 件の予定案を作りました。`);
  } catch (error) {
    showToast(error.message, "error");
  }
}

function autoScheduleInput() {
  const state = getState();
  return {
    start_date: state.selectedDate,
    days: 7,
    timezone: state.preferences?.timezone || state.settings.timezone,
    named_hours: [],
  };
}

async function runAutoSchedule(apply) {
  try {
    if (!apply) {
      const preview = await api.previewAutoSchedule(autoScheduleInput());
      updateState({ autoPreview: preview });
      showToast(`${preview.changed_count} 件の変更差分を作りました。`);
      return;
    }
    const preview = getState().autoPreview;
    if (!preview?.fingerprint) {
      showToast("先に7日プランを Preview してください。", "error");
      return;
    }
    const result = await api.applyAutoSchedule({
      ...autoScheduleInput(),
      fingerprint: preview.fingerprint,
    });
    const schedule = await api.schedule(getState().selectedDate);
    updateState({ schedule: schedule.blocks, autoPreview: null, preview: null });
    showToast(`${result.blocks.length} 件を fingerprint 確認後に適用しました。`);
  } catch (error) {
    if (error.status === 409) {
      updateState({ autoPreview: null });
      showToast("予定が変わりました。再度 Preview してください。", "error");
    } else {
      showToast(error.message, "error");
    }
  }
}

async function saveTask(form) {
  const data = new FormData(form);
  const id = data.get("id");
  const estimate = data.get("estimated_minutes");
  const payload = {
    title: String(data.get("title") || "").trim(),
    description: String(data.get("description") || "").trim() || null,
    due_date: data.get("due_date") || null,
    estimated_minutes: estimate ? Number(estimate) : null,
  };
  if (id) payload.completed = data.get("completed") === "on";
  const submit = form.querySelector("[type='submit']");
  setControlBusy(submit, true);
  try {
    if (id) await api.updateTask(id, payload);
    else await api.createTask(payload);
    closeTaskDialog();
    await refreshData({ quiet: true });
    showToast(id ? "タスクを更新しました。" : "Inbox に追加しました。");
  } catch (error) {
    showToast(error.message, "error");
  } finally {
    setControlBusy(submit, false);
  }
}

async function toggleTask(id, checkbox) {
  const task = getState().tasks.find((candidate) => candidate.id === id);
  if (!task) return;
  checkbox.disabled = true;
  try {
    await api.updateTask(id, { title: task.title, description: task.description, due_date: task.due_date, estimated_minutes: task.estimated_minutes, completed: !task.completed });
    await refreshData({ quiet: true });
    showToast(task.completed ? "タスクを未完了に戻しました。" : "タスクを完了しました。");
  } catch (error) {
    checkbox.checked = task.completed;
    showToast(error.message, "error");
  } finally {
    if (checkbox.isConnected) checkbox.disabled = false;
  }
}

async function deleteTask(id) {
  const task = getState().tasks.find((candidate) => candidate.id === id);
  if (!task || !window.confirm(`「${task.title}」を削除しますか？`)) return;
  try {
    await api.deleteTask(id);
    await refreshData({ quiet: true });
    showToast("タスクを削除しました。");
  } catch (error) {
    showToast(error.message, "error");
  }
}

async function saveHabit(form) {
  const data = new FormData(form);
  const id = String(data.get("id") || "");
  const preferredStart = String(data.get("preferredStart") || "");
  const preferredEnd = String(data.get("preferredEnd") || "");
  if (Boolean(preferredStart) !== Boolean(preferredEnd)) {
    showToast("希望時間は開始・終了を両方入力してください。", "error");
    return;
  }
  const scheduleKind = String(data.get("scheduleKind"));
  const weekdays = data.getAll("weekdays").map(String);
  if (scheduleKind === "WEEKDAYS" && !weekdays.length) {
    showToast("曜日を1つ以上選択してください。", "error");
    return;
  }
  const payload = {
    title: String(data.get("title") || "").trim(),
    schedule: { kind: scheduleKind, weekdays: scheduleKind === "WEEKDAYS" ? weekdays : [] },
    duration_minutes: Number(data.get("durationMinutes")),
    preferred_window: preferredStart ? { start: preferredStart, end: preferredEnd } : null,
    flexibility: String(data.get("flexibility")),
  };
  const submit = form.querySelector("[type='submit']");
  setControlBusy(submit, true);
  try {
    if (id) await habits.update(id, payload);
    else await habits.add(payload);
    closeHabitDialog();
    await refreshData({ quiet: true });
    showToast(id ? "Habit を更新しました。" : "Habit を追加しました。");
  } catch (error) {
    showToast(error.message, "error");
  } finally {
    setControlBusy(submit, false);
  }
}

async function expandHabits() {
  try {
    const start = getState().selectedDate;
    const response = await habits.expand(start, addDays(start, 7));
    updateState({ occurrences: response.occurrences });
    showToast(`${response.occurrences.length} 件の occurrence を確認しました。`);
  } catch (error) {
    showToast(error.message, "error");
  }
}

async function disableHabit(id) {
  const habit = getState().habits.find((item) => item.id === id);
  if (!habit || !window.confirm(`「${habit.title}」を無効にしますか？`)) return;
  try {
    await habits.disable(id);
    await refreshData({ quiet: true });
    updateState({ occurrences: getState().occurrences.filter((item) => item.habit_id !== id) });
    showToast("Habit を無効にしました。");
  } catch (error) {
    showToast(error.message, "error");
  }
}

async function skipOccurrence(id) {
  try {
    await habits.skip(id, "Skipped from Web");
    await expandHabits();
  } catch (error) {
    showToast(error.message, "error");
  }
}

async function snoozeOccurrence(id) {
  const initial = new Date(Date.now() + 60 * 60 * 1000).toISOString().slice(0, 16);
  const value = window.prompt("Snooze until（例: 2026-08-15T18:00）", initial);
  if (!value) return;
  const until = new Date(value);
  if (Number.isNaN(until.valueOf())) {
    showToast("日時を解釈できません。", "error");
    return;
  }
  try {
    await habits.snooze(id, until.toISOString());
    await expandHabits();
  } catch (error) {
    showToast(error.message, "error");
  }
}

async function connectCalendar() {
  try {
    const accessMode = document.querySelector("[data-calendar-access]")?.value || "read_write";
    const response = await calendar.startOAuth(accessMode);
    window.location.assign(response.authorization_url);
  } catch (error) {
    showToast(error.message, "error");
  }
}

async function loadCalendars(accountId) {
  try {
    const response = await calendar.calendars(accountId);
    updateState({ remoteCalendars: { ...getState().remoteCalendars, [accountId]: response.calendars } });
  } catch (error) {
    showToast(error.message, "error");
  }
}

async function saveCalendarSelection(accountId) {
  const ids = [...document.querySelectorAll(`[data-calendar-choice='${accountId}']:checked`)].map((input) => input.value);
  try {
    await calendar.saveSelection(accountId, ids);
    await refreshData({ quiet: true });
    showToast(`${ids.length} 個の Calendar を選択しました。`);
  } catch (error) {
    showToast(error.message, "error");
  }
}

function calendarRange() {
  const start = getState().selectedDate;
  return { start_date: start, end_date_exclusive: addDays(start, 8) };
}

async function syncCalendar(accountId) {
  try {
    const result = await calendar.sync(accountId, calendarRange());
    showToast(`${result.calendars_synced} Calendar / ${result.events_upserted} events を同期しました。`);
    await refreshData({ quiet: true });
  } catch (error) {
    showToast(error.message, "error");
  }
}

async function ensureManagedCalendar(accountId) {
  try {
    await calendar.ensureManaged(accountId);
    await refreshData({ quiet: true });
    showToast("Mnema managed calendar を準備しました。");
  } catch (error) {
    showToast(error.message, "error");
  }
}

async function writebackCalendar(accountId) {
  try {
    const result = await calendar.writeback(accountId, calendarRange());
    showToast(`書き戻し: ${result.created} 作成 / ${result.updated} 更新 / ${result.deleted} 削除`);
  } catch (error) {
    showToast(error.message, "error");
  }
}

async function saveSchedulingSettings(form) {
  const data = new FormData(form);
  const workStart = String(data.get("workStart"));
  const workEnd = String(data.get("workEnd"));
  const sleepStart = String(data.get("sleepStart"));
  const sleepEnd = String(data.get("sleepEnd"));
  if (workStart >= workEnd) {
    showToast("Work end は start より後にしてください。", "error");
    return;
  }
  if (sleepStart === sleepEnd) {
    showToast("Sleep start と end は異なる時刻にしてください。", "error");
    return;
  }
  const weekdays = ["MONDAY", "TUESDAY", "WEDNESDAY", "THURSDAY", "FRIDAY"];
  const everyDay = [...weekdays, "SATURDAY", "SUNDAY"];
  const rangeDays = (days, start, end) => days.map((weekday) => ({ weekday, ranges: [{ start, end }] }));
  const retainedHours = (getState().preferences?.named_hours || [])
    .filter((policy) => policy.name.toLowerCase() !== "work");
  const payload = {
    timezone: String(data.get("timezone") || "").trim(),
    named_hours: [...retainedHours, { name: "work", hard: false, days: rangeDays(weekdays, workStart, workEnd) }],
    sleep: { name: "sleep", hard: true, days: rangeDays(everyDay, sleepStart, sleepEnd) },
    default_travel_buffer_minutes: Number(data.get("travelBuffer")),
  };
  const submit = form.querySelector("[type='submit']");
  setControlBusy(submit, true);
  try {
    const preferences = await api.updatePreferences(payload);
    saveSettings({
      timezone: preferences.timezone,
      planningStart: workStart,
      planningEnd: workEnd,
      refreshSeconds: Number(data.get("refreshSeconds")),
      theme: String(data.get("theme")),
    });
    updateState({ preferences, preview: null, autoPreview: null });
    configureAutoRefresh();
    showToast("Hours / Sleep / Travel を保存しました。");
  } catch (error) {
    showToast(error.message, "error");
  } finally {
    setControlBusy(submit, false);
  }
}

document.addEventListener("click", (event) => {
  const target = event.target.closest("[data-action]");
  if (!target) return;
  const action = target.dataset.action;
  if (action === "new-task") return openTaskDialog();
  if (action === "close-dialog") return closeTaskDialog();
  if (action === "new-habit") return openHabitDialog();
  if (action === "close-habit-dialog") return closeHabitDialog();
  if (action === "edit-task") {
    const task = getState().tasks.find((candidate) => candidate.id === target.dataset.id);
    if (task) return openTaskDialog(task);
  }
  if (action === "edit-habit") {
    const habit = getState().habits.find((candidate) => candidate.id === target.dataset.id);
    if (habit) return openHabitDialog(habit);
  }
  const actions = {
    "preview-plan": () => runPlan(false),
    "apply-plan": () => runPlan(true),
    "preview-auto": () => runAutoSchedule(false),
    "apply-auto": () => runAutoSchedule(true),
    "expand-habits": expandHabits,
    "connect-calendar": connectCalendar,
    "delete-task": () => deleteTask(target.dataset.id),
    "disable-habit": () => disableHabit(target.dataset.id),
    "skip-occurrence": () => skipOccurrence(target.dataset.id),
    "snooze-occurrence": () => snoozeOccurrence(target.dataset.id),
    "load-calendars": () => loadCalendars(target.dataset.id),
    "save-calendar-selection": () => saveCalendarSelection(target.dataset.id),
    "sync-calendar": () => syncCalendar(target.dataset.id),
    "ensure-managed": () => ensureManagedCalendar(target.dataset.id),
    "writeback-calendar": () => writebackCalendar(target.dataset.id),
    "retry-refresh": () => refreshData(),
  };
  if (actions[action]) runBusyAction(target, actions[action]);
});

document.addEventListener("change", (event) => {
  const target = event.target;
  if (target.matches("[data-action='select-date']")) {
    updateState({ selectedDate: target.value, preview: null, autoPreview: null });
    refreshSchedule();
  }
  if (target.matches("[data-action='toggle-task']")) toggleTask(target.dataset.id, target);
  if (target.matches("[name='scheduleKind']")) syncHabitWeekdayAvailability();
});

document.addEventListener("submit", (event) => {
  if (event.target.id === "settings-form") {
    event.preventDefault();
    saveSchedulingSettings(event.target);
  }
});

getTaskForm().addEventListener("submit", (event) => {
  event.preventDefault();
  saveTask(event.currentTarget);
});
getHabitForm().addEventListener("submit", (event) => {
  event.preventDefault();
  saveHabit(event.currentTarget);
});

document.querySelector("#refresh-button").addEventListener("click", () => refreshData());
document.querySelector("#theme-button").addEventListener("click", () => {
  saveSettings({ theme: getState().settings.theme === "dark" ? "light" : "dark" });
});
window.addEventListener("hashchange", () => {
  updateState({ route: location.hash.replace("#", "") || "home" });
  requestAnimationFrame(() => document.querySelector("#view-title").focus());
});
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") refreshData({ quiet: true });
});
document.querySelector("#task-dialog").addEventListener("cancel", (event) => { event.preventDefault(); closeTaskDialog(); });
document.querySelector("#habit-dialog").addEventListener("cancel", (event) => { event.preventDefault(); closeHabitDialog(); });

async function runBusyAction(control, action) {
  if (control.disabled || control.getAttribute("aria-busy") === "true") return;
  setControlBusy(control, true);
  try {
    await action();
  } catch (error) {
    showToast(error?.message || "操作を完了できませんでした。", "error");
  } finally {
    setControlBusy(control, false);
  }
}

function setControlBusy(control, busy) {
  if (!control) return;
  control.disabled = busy;
  if (busy) control.setAttribute("aria-busy", "true");
  else control.removeAttribute("aria-busy");
}

function timezoneOffsetForDate(timezone, value, fallback) {
  try {
    const [year, month, day] = value.split("-").map(Number);
    const utcNoon = Date.UTC(year, month - 1, day, 12);
    const initialOffset = offsetMinutesAt(new Date(utcNoon), timezone);
    const localNoonInstant = new Date(utcNoon - initialOffset * 60_000);
    const offset = offsetMinutesAt(localNoonInstant, timezone);
    const sign = offset >= 0 ? "+" : "-";
    const absolute = Math.abs(offset);
    return `${sign}${String(Math.floor(absolute / 60)).padStart(2, "0")}:${String(absolute % 60).padStart(2, "0")}`;
  } catch (_) {
    return fallback;
  }
}

function offsetMinutesAt(date, timezone) {
  const parts = new Intl.DateTimeFormat("en-CA", {
    timeZone: timezone,
    hourCycle: "h23",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).formatToParts(date);
  const values = Object.fromEntries(parts.map((part) => [part.type, part.value]));
  const representedUtc = Date.UTC(
    Number(values.year),
    Number(values.month) - 1,
    Number(values.day),
    Number(values.hour),
    Number(values.minute),
    Number(values.second),
  );
  return Math.round((representedUtc - date.getTime()) / 60_000);
}

function addDays(value, days) {
  const [year, month, day] = value.split("-").map(Number);
  const date = new Date(Date.UTC(year, month - 1, day + days));
  return date.toISOString().slice(0, 10);
}

initialize();
