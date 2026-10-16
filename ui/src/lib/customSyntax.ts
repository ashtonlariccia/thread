/**
 * Languages of the user's own: a tokeniser built from a description in the
 * config (`[syntax.<name>]`) rather than from a grammar shipped with Tide.
 *
 * The description is lists of words and a few delimiters, which is what most
 * of a language's colouring comes down to, and regular expressions for the
 * rest. What is built from it is a line tokeniser, the same kind the legacy
 * modes in `syntax.ts` are: it colours, and knows nothing of structure.
 */
import { StreamLanguage, StringStream, type StreamParser } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { tags as t } from "@lezer/highlight";

/** One language, as the config has it. */
export type SyntaxDef = {
  /** Without the dot, lower-case. */
  extensions: string[];
  line_comment: string;
  /** What opens and what closes one: two, or none. */
  block_comment: string[];
  strings: string[];
  /** What opens and what closes code inside a string: two, or none. */
  interpolation: string[];
  keywords: string[];
  types: string[];
  constants: string[];
  functions: string[];
  patterns: SyntaxPattern[];
};

export type SyntaxPattern = { match: string; as: string };

export const EMPTY_SYNTAX: SyntaxDef = {
  extensions: [],
  line_comment: "",
  block_comment: [],
  strings: [],
  interpolation: [],
  keywords: [],
  types: [],
  constants: [],
  functions: [],
  patterns: [],
};

/** What a pattern's matches can be coloured as, and the token each one is. */
const TOKENS = {
  keyword: "keyword",
  type: "typeName",
  constant: "constant",
  function: "call",
  builtin: "builtin",
  operator: "operator",
  string: "string",
  comment: "comment",
  number: "number",
  property: "propertyName",
} as const;

export const CATEGORIES = Object.keys(TOKENS) as (keyof typeof TOKENS)[];

/** Why a pattern cannot be used, or null if it can. */
export function patternError(source: string): string | null {
  try {
    new RegExp(source);
    return null;
  } catch (e) {
    return e instanceof Error ? e.message : String(e);
  }
}

type State = {
  /** Inside a block comment that an earlier line opened. */
  comment: boolean;
  /** Inside a string that an earlier line opened: what will close it. */
  string: string | null;
  /**
   * The part of a pattern's match that is to be coloured, further along
   * this line: the columns it runs between, and what it is coloured as.
   */
  capture: { from: number; to: number; token: string } | null;
  /**
   * Inside code inside a string: what will close the string once the code
   * has ended, and how many of the code's openers are still to be closed.
   */
  embed: string | null;
  depth: number;
};

const NUMBER = /^(?:0[xX][\da-fA-F_]+|0[bB][01_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)/;
const WORD = /^[A-Za-z_][\w]*/;
const OPERATOR = /^[+\-*/%=<>!&|^~?:@$#\\]+/;

export function customParser(def: SyntaxDef): StreamParser<State> {
  const words = new Map<string, string>();
  // Later lists win, so a word in two of them is the more specific thing.
  for (const word of def.functions) words.set(word, TOKENS.function);
  for (const word of def.types) words.set(word, TOKENS.type);
  for (const word of def.constants) words.set(word, TOKENS.constant);
  for (const word of def.keywords) words.set(word, TOKENS.keyword);

  // Longest first: `"""` has to be tried before `"`.
  const strings = [...def.strings].filter(Boolean).sort((a, b) => b.length - a.length);
  const [open, close] = def.block_comment.length === 2 ? def.block_comment : [null, null];
  const [codeOpen, codeClose] =
    def.interpolation?.length === 2 && def.interpolation.every(Boolean)
      ? def.interpolation
      : [null, null];

  const patterns = def.patterns.flatMap((pattern) => {
    const token = TOKENS[pattern.as as keyof typeof TOKENS];
    if (!token || patternError(pattern.match) !== null) return [];
    // Tried where a token starts, so it has to match from there. With `d`,
    // for where in the match a group of it was.
    return [{ regex: new RegExp(`^(?:${pattern.match})`, "d"), token }];
  });

  /** Read to the end of a string, or of the line if it does not end on it. */
  function string(stream: StringStream, state: State, quote: string) {
    // Not from a string that is itself inside a string's code: one level.
    const embeds = codeOpen !== null && state.embed === null;
    while (!stream.eol()) {
      if (stream.match(quote)) {
        state.string = null;
        return TOKENS.string;
      }
      if (embeds && stream.match(codeOpen!, false)) {
        // What there is of the string so far goes out first, as string.
        state.string = quote;
        if (stream.pos > stream.start) return TOKENS.string;
        stream.match(codeOpen!);
        state.string = null;
        state.embed = quote;
        state.depth = 1;
        return "bracket";
      }
      // A backslash takes whatever follows it, a quote included.
      if (stream.next() === "\\") stream.next();
    }
    // Only a long delimiter carries on to the next line: an unclosed `"` is
    // a typo being made, and should not swallow the rest of the file.
    state.string = quote.length > 1 ? quote : null;
    return TOKENS.string;
  }

  function comment(stream: StringStream, state: State) {
    const end = close === null ? -1 : stream.string.indexOf(close, stream.pos);
    if (end === -1) {
      stream.skipToEnd();
      state.comment = true;
    } else {
      stream.pos = end + close!.length;
      state.comment = false;
    }
    return TOKENS.comment;
  }

  /** One token, by everything but a group still to come. */
  function read(stream: StringStream, state: State): string | null {
    if (state.comment) return comment(stream, state);
    if (state.string !== null) return string(stream, state, state.string);
    if (stream.eatSpace()) return null;

    // In a string's code, which ends where its opener is closed, and the
    // string carries on from there.
    if (state.embed !== null) {
      if (stream.match(codeClose!)) {
        if (--state.depth === 0) {
          state.string = state.embed;
          state.embed = null;
        }
        return "bracket";
      }
      if (stream.match(codeOpen!)) {
        state.depth++;
        return "bracket";
      }
    }

    if (def.line_comment && stream.match(def.line_comment)) {
      stream.skipToEnd();
      return TOKENS.comment;
    }
    if (open !== null && stream.match(open)) return comment(stream, state);
    for (const quote of strings) {
      if (stream.match(quote)) return string(stream, state, quote);
    }

    for (const { regex, token } of patterns) {
      const found = stream.match(regex, false) as RegExpMatchArray | null;
      // One that matches nothing would never move on.
      if (!found || found[0].length === 0) continue;
      // A pattern with a group in it colours the group and not the rest:
      // `ds\s+(\w+)` is the name after a `ds`, and the `ds` is whatever
      // it would have been anyway.
      const group = found.indices?.slice(1).find((at) => at && at[1] > at[0]);
      if (!group) {
        stream.pos += found[0].length;
        return token;
      }
      if (group[0] === 0) {
        stream.pos += group[1];
        return token;
      }
      // Further along: what is before it is read as usual, up to it.
      state.capture = { from: stream.pos + group[0], to: stream.pos + group[1], token };
      break;
    }

    if (stream.match(NUMBER)) return TOKENS.number;
    const word = stream.match(WORD) as RegExpMatchArray | null;
    if (word) {
      const known = words.get(word[0]);
      if (known) return known;
      // A name with a bracket after it is being called.
      return stream.peek() === "(" ? TOKENS.function : "variableName";
    }
    if (stream.match(OPERATOR)) return TOKENS.operator;

    const char = stream.next();
    if (char && "()[]{}".includes(char)) return "bracket";
    if (char && ",.;".includes(char)) return "punctuation";
    return null;
  }

  return {
    startState: () => ({ comment: false, string: null, capture: null, embed: null, depth: 0 }),

    token(stream, state) {
      // Only ever on the line it was found on.
      if (stream.sol()) {
        state.capture = null;
        // As with the string it is in: only a long delimiter's runs on.
        if (state.embed !== null && state.embed.length < 2) state.embed = null;
      }
      const capture = state.capture;
      if (capture && stream.pos >= capture.from) {
        state.capture = null;
        if (stream.pos === capture.from) {
          stream.pos = capture.to;
          return capture.token;
        }
      }

      const token = read(stream, state);
      // Nothing before a group runs on into it.
      const ahead = state.capture;
      if (ahead && stream.pos > ahead.from) stream.pos = ahead.from;
      return token;
    },

    languageData: {
      commentTokens: {
        line: def.line_comment || undefined,
        block: open !== null && close !== null ? { open, close } : undefined,
      },
    },

    // The tokens that are not simply the name of a tag.
    tokenTable: {
      constant: t.constant(t.variableName),
      call: t.function(t.variableName),
      builtin: t.macroName,
    },
  };
}

export function customLanguage(def: SyntaxDef): Extension {
  return StreamLanguage.define(customParser(def));
}

/** The token each stretch of some lines comes out as, for tests: `[text, token]`. */
export function tokens(def: SyntaxDef, lines: string[]): [string, string | null][][] {
  const parser = customParser(def);
  const state = parser.startState!(2);
  return lines.map((line) => {
    const out: [string, string | null][] = [];
    const stream = new StringStream(line, 2, 2);
    while (!stream.eol()) {
      stream.start = stream.pos;
      const token = parser.token(stream, state);
      const text = line.slice(stream.start, stream.pos);
      if (text.trim()) out.push([text, token]);
    }
    return out;
  });
}
