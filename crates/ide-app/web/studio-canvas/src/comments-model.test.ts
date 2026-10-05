import test from "node:test";
import assert from "node:assert/strict";
import { commentAnchor, commentBodyValid, commentDate, commentPopoverPosition, pinPosition } from "./comments-model.ts";

test("pins track a screen through moving, resizing, and zooming", () => {
  const board = { x: 100, y: -200, width: 400, height: 800 };
  const pin = commentAnchor({ x: 200, y: 400 }, board)!;
  assert.deepEqual(pin, { x: .25, y: .75 });
  assert.deepEqual(pinPosition(pin, { x: -500, y: 300, width: 800, height: 400 }, { x: 150, y: 10, zoom: .5 }), { x: 0, y: 310 });
});
test("screen titles and invalid positions cannot place a comment", () => {
  const board = { x: 10, y: 20, width: 400, height: 800 };
  for (const point of [{ x: 40, y: 19 }, { x: 411, y: 40 }, { x: NaN, y: 40 }]) assert.equal(commentAnchor(point, board), null);
  assert.equal(commentAnchor({ x: 10, y: 20 }, { ...board, width: 0 }), null);
});
test("comment limits count UTF-8 bytes and reject empty text", () => {
  assert.equal(commentBodyValid(" \n "), false);
  assert.equal(commentBodyValid("x".repeat(8000)), true);
  assert.equal(commentBodyValid("x".repeat(8001)), false);
  assert.equal(commentBodyValid("😀".repeat(2000)), true);
  assert.equal(commentBodyValid("😀".repeat(2001)), false);
});
test("damaged timestamps cannot throw when displaying a saved comment", () => {
  assert.equal(commentDate(0)?.toISOString(), "1970-01-01T00:00:00.000Z");
  assert.equal(commentDate(1791140000)?.toISOString(), "2026-10-04T18:53:20.000Z");
  for (const value of [Number.MAX_VALUE, 18446744073709551615, Infinity, NaN, -1]) {
    assert.equal(commentDate(value), null);
  }
});

test("a pin's popover flips and stays reachable at stage edges", () => {
  const size = { width: 320, height: 240 }, area = { width: 800, height: 600 };
  assert.deepEqual(commentPopoverPosition({ x: 100, y: 100 }, size, area), { x: 144, y: 68 });
  assert.deepEqual(commentPopoverPosition({ x: 750, y: 590 }, size, area), { x: 418, y: 348 });
  assert.deepEqual(commentPopoverPosition({ x: -100, y: -100 }, size, area), { x: 12, y: 12 });
  assert.deepEqual(commentPopoverPosition({ x: 290, y: 600 }, { width: 266, height: 500 }, { width: 320, height: 720 }), { x: 12, y: 208 });
  assert.deepEqual(commentPopoverPosition({ x: 160, y: 100 }, { width: 296, height: 200 }, { width: 320, height: 720 }), { x: 12, y: 112 });
  assert.deepEqual(commentPopoverPosition({ x: 160, y: 690 }, { width: 296, height: 200 }, { width: 320, height: 720 }), { x: 12, y: 446 });
});
