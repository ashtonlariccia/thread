/**
 * What this window is connected to, and the connections that are saved.
 *
 * A window is on this machine or on one remote. The backend holds the
 * connection itself and every secret that goes with it; this is what the
 * window needs to draw the indicator and the Remote menu.
 */
import { invoke } from "@tauri-apps/api/core";

/** A connection, as the backend describes it. */
export type RemoteInfo = {
  /** `user@host`. */
  label: string;
  user: string;
  host: string;
  /** The folder the server puts us in. */
  home: string;
  /** Its id, if it is one of the saved connections. */
  saved: string | null;
};

export type SavedConnection = { id: string; host: string; port: number; username: string };

export type Auth =
  | { kind: "password"; password: string }
  | { kind: "key"; path: string; passphrase: string };

/** What the connect dialog asks for. */
export type ConnectRequest = { host: string; port: number; username: string; auth: Auth };

/** A private key found in `~\.ssh`. */
export type DiscoveredKey = { path: string; name: string; encrypted: boolean };

/** Why a connection could not be made. Mirrors `ConnectError` in the core. */
export type ConnectError = {
  kind: "auth" | "unreachable" | "session" | "unknownHostKey" | "hostKeyChanged";
  message: string;
  fingerprint?: string;
  expectedFingerprint?: string;
};

/** `user@host`, with the port when it is not the usual one. */
export function connectionLabel(c: { username: string; host: string; port: number }): string {
  return c.port === 22 ? `${c.username}@${c.host}` : `${c.username}@${c.host}:${c.port}`;
}

export type RemoteStatus =
  /** On this machine. */
  | "local"
  | "connecting"
  | "connected"
  /** Was connected, and the connection has died. What is open stays open. */
  | "lost";

export class RemoteStore {
  status = $state<RemoteStatus>("local");
  info = $state.raw<RemoteInfo | null>(null);
  /** Who is being connected to, while it is. */
  pending = $state<string | null>(null);
  known = $state.raw<SavedConnection[]>([]);

  /** Files and terminals are the remote's: connected, or was until it dropped. */
  get remote(): boolean {
    return this.status === "connected" || this.status === "lost";
  }

  async refreshKnown() {
    try {
      this.known = await invoke<SavedConnection[]>("remote_known");
    } catch (e) {
      console.error("remote_known failed", e);
    }
  }

  async forget(id: string) {
    try {
      this.known = await invoke<SavedConnection[]>("remote_forget", { id });
      // The connection being used is still up; it is just no longer saved.
      if (this.info?.saved === id) this.info = { ...this.info, saved: null };
    } catch (e) {
      console.error("remote_forget failed", e);
    }
  }
}
