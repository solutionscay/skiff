import { h } from "../ui/dom";
import { rune } from "../ui/runes";
import { picker, smallButton, textBox } from "../ui/controls";
import { t } from "../i18n";

/**
 * A Settings table: Settings › Agents and Settings › Task types. One
 * implementation, so they edit, remove and report errors the
 * same way. A new row comes from the page's own Add… action.
 *
 * - A text cell saves on Enter or blur, Esc puts the old value back, and a
 *   picker saves on pick. A rename to an empty or taken name reverts, and the
 *   error line says why.
 * - − removes at once. The keys go to the − now at the same place, else the
 *   last one.
 * - The error lives in the caller's {@link RowTableState}, so a redraw for any
 *   other reason keeps it.
 */

/** One column. `key` names the field in edits. */
interface RowCol<R> {
  key: string;
  /** Column header text. Empty for the lead column. */
  head: string;
  /** The word in aria-labels: "name", "model". */
  name: string;
  /** lead: a cell the caller draws. key: the name, required, renames. select: a picker. */
  kind: "lead" | "key" | "text" | "select";
  mono?: boolean;
  placeholder?: (row: R) => string;
  /** select: [value, label] pairs for a row. */
  options?: (row: R) => [string, string][];
  /** select: the one line shown, off, when there are none. */
  empty?: string;
  /** Text in place of the field, such as "set by agent". Null: the field shows. */
  note?: (row: R) => string | null;
  get?: (row: R) => string;
  /** lead: the caller's control for the row. */
  draw?: (row: R) => HTMLElement;
  /** Why a value cannot be saved, checked before the Host. Null: good. */
  check?: (value: string) => string | null;
}

export interface RowTableSpec<R> {
  /** The CSS class that sets the columns. */
  grid: string;
  /** The id of the section's heading. */
  labelledBy: string;
  /** No head row: the table takes its heads from the table above it. */
  noHead?: boolean;
  /** Header of the end column. Empty: Remove, for screen readers only. */
  endHead?: string;
  cols: RowCol<R>[];
  rows: R[];
  id: (row: R) => string;
  rowClass?: (row: R) => string;
  /** What shows in the end cell before the −: the agent's status, or the row's own buttons. */
  status?: (row: R) => Node;
  /** What keeps a row's − off, short: "Admiral +3". The − says it, by tooltip, by description and when pressed. Null: it can go. */
  kept?: (row: R) => string | null;
  /** Why a row's new value cannot be saved, past the column's own check. */
  checkEdit?: (row: R, key: string, value: string) => string | null;
  /** Each saves and answers with the error to show, or null. They do not redraw. */
  edit?: (row: R, key: string, value: string) => Promise<string | null>;
  remove?: (row: R) => Promise<string | null>;
}

/** What survives a redraw. The caller keeps one per table. */
export interface RowTableState {
  error: string | null;
}

/**
 * A section head inside a Settings page: one 40px tinted band, the title
 * left, a count right.
 */
export function sectionHead(title: string, id?: string, count?: string): HTMLElement {
  const head = h("div", "set-band");
  const band = h("span", "set-band-title", title);
  if (id) band.id = id;
  head.append(band);
  if (count !== undefined) head.append(h("span", "set-band-count", count));
  return head;
}

export const tableState = (): RowTableState => ({ error: null });

export function rowTable<R>(spec: RowTableSpec<R>, state: RowTableState, redraw: () => void): HTMLElement {
  const table = h("div", `set-table ${spec.grid}`);
  table.dataset.rowTable = spec.grid;
  table.setAttribute("role", "table");
  table.setAttribute("aria-labelledby", spec.labelledBy);

  function settle(error: string | null) {
    state.error = error;
    redraw();
  }

  if (!spec.noHead) {
    const head = h("div", "set-row set-cols");
    head.setAttribute("role", "row");
    for (const c of spec.cols) {
      const cell = h("span", cellClass(c), c.head);
      cell.setAttribute("role", "columnheader");
      if (!c.head) cell.setAttribute("aria-label", c.name);
      head.appendChild(cell);
    }
    const endHead = h("span", "c-status c-end", spec.endHead ?? "");
    endHead.setAttribute("role", "columnheader");
    if (!spec.endHead && spec.remove) endHead.setAttribute("aria-label", t("table.remove"));
    head.appendChild(endHead);
    table.appendChild(head);
  }

  spec.rows.forEach((r, at) => {
    const id = spec.id(r);
    const row = h("div", ("set-row " + (spec.rowClass?.(r) ?? "")).trim());
    row.setAttribute("role", "row");
    row.dataset.id = id;
    for (const c of spec.cols) {
      const cell = h("span", cellClass(c));
      cell.setAttribute("role", "cell");
      const label = t("table.cell", { id, field: c.name });
      const note = c.note?.(r);
      if (c.kind === "lead") cell.appendChild(c.draw!(r));
      else if (note !== null && note !== undefined) {
        cell.classList.add("c-note");
        cell.textContent = note;
      } else if (c.kind === "select") {
        const old = c.get?.(r) ?? "";
        const options = c.options?.(r) ?? [];
        cell.appendChild(picker({
          label,
          value: old,
          shows: options.length ? undefined : c.empty ?? "",
          disabled: !options.length,
          entries: () => options.map(([value, text]) => ({ value, label: text })),
          pick: (v) => void commit(r, c, v, old),
        }));
      } else {
        const old = c.get?.(r) ?? "";
        cell.appendChild(textBox({
          label,
          value: old,
          placeholder: c.placeholder?.(r) ?? "",
          mono: c.mono,
          name: c.kind === "key",
          save: (v) => void commit(r, c, v, old),
        }));
      }
      row.appendChild(cell);
    }
    const end = h("span", "c-status c-end c-acts");
    end.setAttribute("role", "cell");
    if (spec.status) end.appendChild(spec.status(r));
    if (spec.remove) {
      const kept = spec.kept?.(r) ?? null;
      const why = kept === null ? null : t("table.usedBy", { who: kept });
      const rm = smallButton(t("table.remove"), () => void (why === null ? remove(r, at) : refuse(why, at)), { icon: rune("indicators-minus"), danger: true, label: t("table.removeRow", { id }) });
      rm.classList.add("c-remove");
      end.appendChild(rm);
      if (why !== null) {
        // aria-disabled, not disabled: the keys and a screen reader must reach the reason.
        rm.setAttribute("aria-disabled", "true");
        rm.title = why;
        const note = h("span", "visually-hidden", why);
        note.id = `${spec.grid}-why-${at}`;
        rm.setAttribute("aria-describedby", note.id);
        end.appendChild(note);
      }
    }
    row.appendChild(end);
    table.appendChild(row);
  });
  if (state.error) {
    const line = h("div", "set-error", state.error);
    line.setAttribute("role", "alert");
    table.appendChild(line);
  }
  return table;

  /** Saves a cell. A bad value goes back to what the row holds, and the line says why. */
  async function commit(r: R, c: RowCol<R>, value: string, old: string) {
    if (value === old) return;
    if (c.kind === "key" && !value) return settle(t("table.needsName"));
    const bad = c.check?.(value) ?? spec.checkEdit?.(r, c.key, value) ?? null;
    if (bad) return settle(c.kind === "key" ? t("table.keepsName", { why: bad, id: spec.id(r) }) : bad);
    settle(await (spec.edit?.(r, c.key, value) ?? null));
  }

  async function remove(r: R, at: number) {
    state.error = (await spec.remove?.(r)) ?? null;
    redraw();
    focusMinus(at);
  }

  /** A press of a − that is off: the error line says why, and nothing changes. */
  function refuse(why: string, at: number) {
    state.error = why;
    redraw();
    focusMinus(at);
  }

  function focusMinus(at: number) {
    const rms = [...document.querySelectorAll<HTMLButtonElement>(`[data-row-table="${spec.grid}"] .c-remove`)];
    rms[Math.min(at, rms.length - 1)]?.focus();
  }
}

function cellClass<R>(c: RowCol<R>): string {
  return c.kind === "lead" ? "c-lead" : `c-field c-${c.key}`;
}
