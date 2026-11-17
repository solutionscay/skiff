import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleFromSource } from "./workspaceHarness.ts";

// The real whenSentence.ts, and the real i18n/time.ts it imports, over the
// real English catalog (navigator: en-US). One formatter for both.
const W = moduleFromSource("../src/appearance/whenSentence.ts", {}, {}, "\nexports.T = require(\"../i18n/time\");");
const T = W.T;

// Intl puts a narrow no-break space before AM/PM.
const plain = (s: string) => s.replace(/\s/g, " ");
// 15:05 UTC on Tue 17 Nov 2026.
const AT = Date.UTC(2026, 10, 17, 15, 5);

test("the chosen zone moves the time; an unknown zone gives the system's", () => {
  T.setTimeDisplay("Asia/Tokyo", "24");
  assert.equal(T.clockTime(AT), "00:05");
  assert.equal(T.shortDate(AT), "Nov 18");
  T.setTimeDisplay("America/New_York", "24");
  assert.equal(T.clockTime(AT), "10:05");
  T.setTimeDisplay("Mars/Olympus", null);
  assert.equal(T.shownZone(), T.systemZone());
});

test("24-hour, 12-hour, and Auto from the language (English: 12-hour)", () => {
  T.setTimeDisplay("UTC", "24");
  assert.equal(T.clockTime(AT), "15:05");
  assert.equal(T.wallClock(9, 0), "09:00");
  T.setTimeDisplay("UTC", "12");
  assert.equal(plain(T.clockTime(AT)), "3:05 PM");
  assert.equal(plain(T.weekdayTime(AT)), "Tue 3:05 PM");
  T.setTimeDisplay("UTC", null);
  assert.equal(T.hour12(), true);
  assert.equal(plain(T.clockTime(AT)), "3:05 PM");
});

test("today shows the time, yesterday says so, older shows the date", () => {
  T.setTimeDisplay("UTC", "24");
  const now = Date.UTC(2026, 10, 17, 18, 0);
  assert.equal(T.recentWhen(AT, now), "15:05");
  assert.equal(T.compactWhen(AT, now), "15:05");
  assert.equal(T.recentWhen(AT - 86_400_000, now), "Yesterday 15:05");
  assert.equal(T.recentWhen(AT - 3 * 86_400_000, now), "Nov 14, 15:05");
  assert.equal(T.compactWhen(AT - 3 * 86_400_000, now), "Nov 14");
  // Ahead: today the time, this week the weekday, later the date.
  assert.equal(T.aheadWhen(AT, now - 3 * 3_600_000), "15:05");
  assert.equal(T.aheadWhen(AT + 86_400_000, now), "Wed 15:05");
  assert.equal(T.aheadWhen(AT + 9 * 86_400_000, now), "Nov 26");
});

test("near midnight the day is the zone's day, not the UTC day", () => {
  // 03:00 UTC on Nov 18 is 22:00 on Nov 17 in Panama (UTC-5).
  T.setTimeDisplay("America/Panama", "24");
  const late = Date.UTC(2026, 10, 18, 3, 0);
  const now = Date.UTC(2026, 10, 17, 20, 0);
  assert.equal(T.daysAgo(late, now), 0);
  assert.equal(T.compactWhen(late, now), "22:00");
  assert.equal(T.fullDate(late), "Nov 17, 2026");
  // The same instant is already tomorrow in Tokyo.
  T.setTimeDisplay("Asia/Tokyo", "24");
  assert.equal(T.fullDate(late), "Nov 18, 2026");
});

test("a typed schedule time reads 24-hour and 12-hour forms", () => {
  assert.equal(String(W.parseTime("21:30")), "21,30");
  assert.equal(String(W.parseTime("9:30 PM")), "21,30");
  assert.equal(String(W.parseTime("12 am")), "0,0");
  assert.equal(String(W.parseTime("9:30 p. m.")), "21,30");
  assert.equal(String(W.parseTime("下午9:30")), "21,30");
  assert.equal(W.parseTime("13 pm"), null);
  assert.equal(W.parseTime("24:00"), null);
  T.setTimeDisplay(null, "12");
  assert.equal(plain(W.hhmm([21, 30])), "9:30 PM");
  T.setTimeDisplay(null, "24");
  assert.equal(W.hhmm([24, 0]), "24:00");
});
