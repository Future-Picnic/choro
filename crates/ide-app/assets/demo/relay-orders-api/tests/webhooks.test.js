import test from "node:test";
import assert from "node:assert/strict";
import { deliverWebhook, resetDeliveryMemory } from "../src/webhooks.js";

test("delivers the same event once per destination", async () => {
  resetDeliveryMemory();
  const calls = [];
  const send = async (...args) => calls.push(args);
  await deliverWebhook({ id: "evt_1" }, "https://store.example.invalid/hook", send);
  const duplicate = await deliverWebhook({ id: "evt_1" }, "https://store.example.invalid/hook", send);
  assert.equal(calls.length, 1);
  assert.equal(duplicate.reason, "duplicate");
});
