export class ApiError extends Error {
  constructor(message, status = 0, code = "request_failed") {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}

async function request(path, options = {}) {
  const response = await fetch(`/api${path}`, {
    ...options,
    headers: {
      Accept: "application/json",
      ...(options.body ? { "Content-Type": "application/json" } : {}),
      ...options.headers,
    },
  });

  if (!response.ok) {
    let body = null;
    try {
      body = await response.json();
    } catch (_) {
      // A proxy may return an HTML error page; use the status fallback below.
    }
    throw new ApiError(
      body?.error?.message || `Request failed (${response.status})`,
      response.status,
      body?.error?.code,
    );
  }
  return response.status === 204 ? null : response.json();
}

export const api = {
  health: () => request("/health"),
  tasks: () => request("/tasks"),
  task: (id) => request(`/tasks/${encodeURIComponent(id)}`),
  createTask: (task) => request("/tasks", { method: "POST", body: JSON.stringify(task) }),
  updateTask: (id, task) => request(`/tasks/${encodeURIComponent(id)}`, {
    method: "PUT",
    body: JSON.stringify(task),
  }),
  deleteTask: (id) => request(`/tasks/${encodeURIComponent(id)}`, { method: "DELETE" }),
  schedule: (date) => request(`/schedule?date=${encodeURIComponent(date)}`),
  previewToday: (input) => request("/plans/today/preview", {
    method: "POST",
    body: JSON.stringify(input),
  }),
  applyToday: (input) => request("/plans/today/apply", {
    method: "POST",
    body: JSON.stringify(input),
  }),
  preferences: () => request("/scheduling/preferences"),
  updatePreferences: (preferences) => request("/scheduling/preferences", {
    method: "PUT",
    body: JSON.stringify(preferences),
  }),
  previewAutoSchedule: (input) => request("/auto-schedule/preview", {
    method: "POST",
    body: JSON.stringify(input),
  }),
  applyAutoSchedule: (input) => request("/auto-schedule/apply", {
    method: "POST",
    body: JSON.stringify(input),
  }),
};

// Feature modules use the same request primitive without coupling to today's UI.
export const featureRequest = request;
