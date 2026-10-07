/**
 * Matching what was typed against a name, loosely: the letters in order,
 * with anything between them. `nt` finds "New Terminal", and so does `term`.
 */

export type Match = {
  /** Higher is a better match. Only for comparing matches of the same query. */
  score: number;
  /** Where in the text each letter typed was found, for marking them. */
  at: number[];
};

const STARTS_WORD = /[\s:/\\_.-]/;

/**
 * How well `text` matches `query`, or null if it does not. Case is ignored,
 * and so are spaces in the query. A letter is looked for at the start of a
 * word first, so initials work, and letters found side by side count for
 * more than the same ones scattered.
 */
export function fuzzy(query: string, text: string): Match | null {
  const wanted = query.toLowerCase().replace(/\s+/g, "");
  const hay = text.toLowerCase();
  const at: number[] = [];
  let score = 0;
  let from = 0;

  for (const letter of wanted) {
    // Carrying on from the last letter beats everything.
    let found = hay[from] === letter && at.length > 0 ? from : -1;
    let bonus = 4;
    if (found === -1) {
      for (let i = hay.indexOf(letter, from); i !== -1; i = hay.indexOf(letter, i + 1)) {
        if (i === 0 || STARTS_WORD.test(hay[i - 1])) {
          found = i;
          break;
        }
      }
      bonus = 3;
    }
    if (found === -1) {
      found = hay.indexOf(letter, from);
      bonus = 0;
    }
    if (found === -1) return null;

    // A little off for what was skipped over, so nearer matches come first.
    score += bonus - (found - from) * 0.05;
    at.push(found);
    from = found + 1;
  }
  // And a little off for length, so the shorter of two equal matches wins.
  return { score: score - text.length * 0.001, at };
}

/**
 * The items a query matches, best first, each with where it matched. An
 * empty query matches everything, in the order it was given.
 */
export function rank<T>(query: string, items: T[], name: (item: T) => string): { item: T; match: Match }[] {
  if (query.trim() === "") return items.map((item) => ({ item, match: { score: 0, at: [] } }));
  const matched = items.flatMap((item) => {
    const match = fuzzy(query, name(item));
    return match ? [{ item, match }] : [];
  });
  // Stable, so equal scores keep the order they came in.
  return matched.sort((a, b) => b.match.score - a.match.score);
}
