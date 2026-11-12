import { Agent, request as httpRequest } from "node:http";
import { connect } from "node:net";
import { expect, it } from "vitest";
import { listenOwned } from "./owned-http-service";

function get(port: number, agent: Agent): Promise<number> {
  return new Promise((resolve, reject) => {
    const outgoing = httpRequest(
      { port, host: "127.0.0.1", agent },
      (reply) => {
        reply.resume();
        reply.on("end", () => resolve(reply.statusCode ?? 0));
      },
    );
    outgoing.on("error", reject);
    outgoing.end();
  });
}

it("ends idle keep-alive sockets when closing", async () => {
  const service = await listenOwned("idle", (_request, response) => {
    response.writeHead(204).end();
  });
  const agent = new Agent({ keepAlive: true });
  expect(await get(service.port, agent)).toBe(204);
  expect(service.snapshot().sockets).toBe(1);
  expect(service.snapshot().inFlight).toBe(0);
  await service.close(1000);
  expect(service.snapshot().sockets).toBe(0);
  agent.destroy();
});

it("waits for an active request that finishes before the deadline", async () => {
  let release: () => void = () => undefined;
  const service = await listenOwned("active", (_request, response) => {
    release = () => response.writeHead(204).end();
  });
  const agent = new Agent({ keepAlive: false });
  const pending = get(service.port, agent);
  await expect.poll(() => service.snapshot().inFlight).toBe(1);
  const closing = service.close(2000);
  setTimeout(() => release(), 100);
  await closing;
  expect(await pending).toBe(204);
  expect(service.snapshot().sockets).toBe(0);
});

it("fails loudly with counts when a request outlives the deadline", async () => {
  const service = await listenOwned("stuck", () => undefined);
  const agent = new Agent({ keepAlive: false });
  const pending = get(service.port, agent).catch(() => 0);
  await expect.poll(() => service.snapshot().inFlight).toBe(1);
  await expect(service.close(200)).rejects.toThrow(
    /stuck.*stage=listening.*sockets=1.*inFlight=1/,
  );
  expect(service.snapshot().sockets).toBe(0);
  await pending;
});

it("ends a connection that never sent a request instead of waiting for a header timeout", async () => {
  const service = await listenOwned("unused", (_request, response) => {
    response.writeHead(204).end();
  });
  const socket = connect(service.port, "127.0.0.1");
  socket.on("error", () => undefined);
  await new Promise<void>((resolve) => socket.once("connect", resolve));
  await expect.poll(() => service.snapshot().sockets).toBe(1);
  const started = Date.now();
  await service.close(5000);
  expect(Date.now() - started).toBeLessThan(2000);
  expect(service.snapshot().sockets).toBe(0);
});
