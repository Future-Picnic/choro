import test from "node:test";
import assert from "node:assert/strict";
import { orderTotal, validateOrder } from "../src/orders.js";

test("validates required order fields", () => {
  assert.equal(validateOrder({}).valid, false);
  assert.equal(validateOrder({ customerId: "cus_demo", items: [{ sku: "mug", quantity: 2 }] }).valid, true);
});

test("calculates totals in minor currency units", () => {
  assert.equal(orderTotal([{ sku: "mug", quantity: 2 }], { mug: { price: 1800 } }), 3600);
});
