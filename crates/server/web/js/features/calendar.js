export const calendarFeature = Object.freeze({
  id: "calendar",
  enabled: true,
  apiBase: "/calendar",
  capabilities: ["oauth", "accounts", "selection", "sync", "managed-calendar", "writeback"],
});

export function createCalendarClient(request) {
  return {
    accounts: () => request(`${calendarFeature.apiBase}/accounts`),
    startOAuth: (accessMode = "read_write") => request(`${calendarFeature.apiBase}/oauth/start`, {
      method: "POST",
      body: JSON.stringify({ access_mode: accessMode }),
    }),
    calendars: (accountId) => request(`${calendarFeature.apiBase}/accounts/${encodeURIComponent(accountId)}/calendars`),
    saveSelection: (accountId, calendarIds) => request(`${calendarFeature.apiBase}/accounts/${encodeURIComponent(accountId)}/selection`, {
      method: "PUT",
      body: JSON.stringify({ calendar_ids: calendarIds }),
    }),
    sync: (accountId, range = {}) => request(`${calendarFeature.apiBase}/accounts/${encodeURIComponent(accountId)}/sync`, {
      method: "POST",
      body: JSON.stringify(range),
    }),
    ensureManaged: (accountId) => request(`${calendarFeature.apiBase}/accounts/${encodeURIComponent(accountId)}/managed-calendar`, {
      method: "POST",
      body: JSON.stringify({ summary: "Mnema Schedule" }),
    }),
    writeback: (accountId, range = {}) => request(`${calendarFeature.apiBase}/accounts/${encodeURIComponent(accountId)}/writeback`, {
      method: "POST",
      body: JSON.stringify(range),
    }),
  };
}
