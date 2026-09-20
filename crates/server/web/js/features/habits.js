export const habitsFeature = Object.freeze({
  id: "habits",
  enabled: true,
  apiBase: "/habits",
  capabilities: ["rules", "occurrences", "skip", "snooze", "disable"],
});

export function createHabitsClient(request) {
  return {
    list: () => request(habitsFeature.apiBase),
    add: (habit) => request(habitsFeature.apiBase, {
      method: "POST",
      body: JSON.stringify(habit),
    }),
    update: (id, habit) => request(`${habitsFeature.apiBase}/${encodeURIComponent(id)}`, {
      method: "PUT",
      body: JSON.stringify(habit),
    }),
    disable: (id) => request(`${habitsFeature.apiBase}/${encodeURIComponent(id)}/disable`, {
      method: "POST",
      body: JSON.stringify({}),
    }),
    expand: (startDate, endDateExclusive) => request(`${habitsFeature.apiBase}/occurrences/expand`, {
      method: "POST",
      body: JSON.stringify({ start_date: startDate, end_date_exclusive: endDateExclusive }),
    }),
    skip: (id, reason = null) => request(`${habitsFeature.apiBase}/occurrences/${encodeURIComponent(id)}/skip`, {
      method: "POST",
      body: JSON.stringify({ reason }),
    }),
    snooze: (id, until) => request(`${habitsFeature.apiBase}/occurrences/${encodeURIComponent(id)}/snooze`, {
      method: "POST",
      body: JSON.stringify({ until }),
    }),
  };
}
