import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import type { AddressInfo, Socket } from "node:net";

export interface OwnedHttpSnapshot {
  label: string;
  stage: string;
  sockets: number;
  inFlight: number;
  elapsedMs: number;
  open: OwnedSocketFacts[];
}

export interface OwnedSocketFacts {
  requestsServed: number;
  bytesRead: number;
  bytesWritten: number;
  ageMs: number;
  idleMs: number;
  readable: boolean;
  writable: boolean;
}

export interface OwnedHttpService {
  port: number;
  stage(name: string): void;
  snapshot(): OwnedHttpSnapshot;
  close(deadlineMs: number): Promise<void>;
}

function describe(snapshot: OwnedHttpSnapshot): string {
  const open = snapshot.open.map((facts) => JSON.stringify(facts)).join(",");
  return `${snapshot.label} stage=${snapshot.stage} sockets=${snapshot.sockets} inFlight=${snapshot.inFlight} elapsedMs=${snapshot.elapsedMs} open=[${open}]`;
}

export async function listenOwned(
  label: string,
  handler: (request: IncomingMessage, response: ServerResponse) => void,
  trace = false,
): Promise<OwnedHttpService> {
  const sockets = new Set<Socket>();
  const served = new Map<Socket, number>();
  const awaiting = new Map<Socket, number>();
  const opened = new Map<Socket, number>();
  const active = new Map<Socket, number>();
  const inFlight = new Set<ServerResponse>();
  const started = Date.now();
  let currentStage = "listening";
  const server = createServer((request, response) => {
    inFlight.add(response);
    awaiting.set(request.socket, (awaiting.get(request.socket) ?? 0) + 1);
    response.once("close", () =>
      awaiting.set(request.socket, (awaiting.get(request.socket) ?? 1) - 1),
    );
    served.set(request.socket, (served.get(request.socket) ?? 0) + 1);
    active.set(request.socket, Date.now());
    response.once("close", () => inFlight.delete(response));
    handler(request, response);
  });
  server.on("connection", (socket) => {
    sockets.add(socket);
    opened.set(socket, Date.now());
    active.set(socket, Date.now());
    socket.once("close", () => sockets.delete(socket));
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const snapshot = (): OwnedHttpSnapshot => ({
    label,
    stage: currentStage,
    sockets: [...sockets].filter((socket) => !socket.destroyed).length,
    inFlight: [...inFlight].filter((response) => !response.writableFinished)
      .length,
    elapsedMs: Date.now() - started,
    open: [...sockets]
      .filter((socket) => !socket.destroyed)
      .map((socket) => ({
        requestsServed: served.get(socket) ?? 0,
        bytesRead: socket.bytesRead,
        bytesWritten: socket.bytesWritten,
        ageMs: Date.now() - (opened.get(socket) ?? Date.now()),
        idleMs: Date.now() - (active.get(socket) ?? Date.now()),
        readable: socket.readable,
        writable: socket.writable,
      })),
  });
  const service: OwnedHttpService = {
    port: (server.address() as AddressInfo).port,
    stage(name) {
      currentStage = name;
      if (trace) process.stderr.write(`[owned-http] ${describe(snapshot())}\n`);
    },
    snapshot,
    async close(deadlineMs) {
      const closed = new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
      const retireQuiet = () => {
        for (const socket of sockets)
          if (!socket.destroyed && (awaiting.get(socket) ?? 0) === 0)
            socket.destroy();
      };
      retireQuiet();
      const sweeper = setInterval(retireQuiet, 20);
      let timer: NodeJS.Timeout | undefined;
      const expired = new Promise<"expired">((resolve) => {
        timer = setTimeout(() => resolve("expired"), deadlineMs);
      });
      const outcome = await Promise.race([
        closed.then(() => "closed"),
        expired,
      ]);
      clearTimeout(timer);
      clearInterval(sweeper);
      if (outcome === "closed") return;
      const failure = new Error(
        `owned service did not retire within ${deadlineMs}ms: ${describe(snapshot())}`,
      );
      server.closeAllConnections();
      await closed.catch(() => undefined);
      throw failure;
    },
  };
  return service;
}
