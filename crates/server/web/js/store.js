const SETTINGS_KEY = "mnema.web.settings.v1";

const systemDark = window.matchMedia?.("(prefers-color-scheme: dark)").matches;

const state = {
  route: location.hash.replace("#", "") || "home",
  health: null,
  tasks: [],
  schedule: [],
  preview: null,
  autoPreview: null,
  preferences: null,
  habits: [],
  occurrences: [],
  calendarAccounts: [],
  remoteCalendars: {},
  selectedDate: localDateString(new Date()),
  loading: false,
  error: null,
  settings: {
    timezone: "Asia/Tokyo",
    timezoneOffset: "+09:00",
    planningStart: "09:00",
    planningEnd: "17:00",
    refreshSeconds: 30,
    theme: systemDark ? "dark" : "light",
  },
};

try {
  Object.assign(state.settings, JSON.parse(localStorage.getItem(SETTINGS_KEY) || "{}"));
} catch (_) {
  // Corrupt browser settings should never prevent the app from opening.
}

const listeners = new Set();

export function getState() {
  return state;
}

export function updateState(patch, render = true) {
  Object.assign(state, patch);
  if (render) listeners.forEach((listener) => listener(state));
}

export function subscribe(listener) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function applyServerDefaults(defaults) {
  if (!defaults || localStorage.getItem(SETTINGS_KEY)) return;
  state.settings = {
    ...state.settings,
    timezone: defaults.timezone,
    timezoneOffset: defaults.timezone_offset,
    planningStart: defaults.planning_start,
    planningEnd: defaults.planning_end,
    refreshSeconds: defaults.refresh_seconds,
  };
}

export function saveSettings(settings) {
  state.settings = { ...state.settings, ...settings };
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(state.settings));
  document.documentElement.dataset.theme = state.settings.theme;
  listeners.forEach((listener) => listener(state));
}

export function localDateString(date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

document.documentElement.dataset.theme = state.settings.theme;
