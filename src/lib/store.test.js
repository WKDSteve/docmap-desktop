import { test } from "node:test";
import assert from "node:assert/strict";
import { createStore } from "./store.js";

test("reading gives the value and assigning stores it", () => {
  const { state } = createStore({ a: 1, b: null });
  assert.equal(state.a, 1);
  state.a = 2;
  assert.equal(state.a, 2);
  assert.deepEqual(Object.keys(state), ["a", "b"]);
});

test("a subscriber hears of a change, with the old value, and reads the new state", () => {
  const store = createStore({ a: 1 });
  const seen = [];
  store.subscribe("a", (value, old, key) => seen.push([value, old, key, store.state.a]));
  store.state.a = 5;
  assert.deepEqual(seen, [[5, 1, "a", 5]]);
});

test("setting the same value tells nobody", () => {
  const store = createStore({ a: 1, n: NaN });
  let calls = 0;
  store.subscribe(["a", "n"], () => (calls += 1));
  store.state.a = 1;
  store.state.n = NaN;
  assert.equal(calls, 0);
  // Objects are compared by identity: a new array is a change.
  const list = createStore({ l: [] });
  list.subscribe("l", () => (calls += 1));
  list.state.l = [];
  assert.equal(calls, 1);
});

test("subscribers run in the order they subscribed, and unsubscribing ends it", () => {
  const store = createStore({ a: 0 });
  const order = [];
  const off = store.subscribe("a", () => order.push("first"));
  store.subscribe("a", () => order.push("second"));
  store.state.a = 1;
  off();
  store.state.a = 2;
  assert.deepEqual(order, ["first", "second", "second"]);
});

test("a subscriber that throws does not stop the others, and the error is not lost", () => {
  const store = createStore({ a: 0 });
  const ran = [];
  store.subscribe("a", () => {
    throw new Error("broken renderer");
  });
  store.subscribe("a", () => ran.push("after"));
  assert.throws(() => (store.state.a = 1), /broken renderer/);
  assert.deepEqual(ran, ["after"]);
  assert.equal(store.state.a, 1);
});

test("a batch reports each changed key once, with the value before and after the whole batch", () => {
  const store = createStore({ a: 1, b: 1, c: 1 });
  const seen = [];
  store.subscribe(["a", "b", "c"], (value, old, key) => seen.push([key, old, value]));
  store.batch(() => {
    store.state.a = 2;
    store.state.a = 3;
    store.state.b = 2;
    store.state.b = 1; // ends where it began
    store.state.c = 9;
  });
  assert.deepEqual(seen, [
    ["a", 1, 3],
    ["c", 1, 9],
  ]);
});

test("patch is a batch, and a key the store does not have is refused", () => {
  const store = createStore({ a: 1, b: 1 });
  const seen = [];
  store.subscribe(["a", "b"], (_v, _o, key) => seen.push(key));
  store.patch({ a: 2, b: 2 });
  assert.deepEqual(seen, ["a", "b"]);
  assert.throws(() => store.patch({ nope: 1 }), /unknown state key: nope/);
  assert.throws(() => store.subscribe("nope", () => {}), /unknown state key: nope/);
  assert.equal(seen.length, 2);
});

test("a nested batch reports once, when the outer one ends", () => {
  const store = createStore({ a: 1 });
  const seen = [];
  store.subscribe("a", (v) => seen.push(v));
  store.batch(() => {
    store.batch(() => {
      store.state.a = 2;
    });
    assert.deepEqual(seen, []);
    store.state.a = 3;
  });
  assert.deepEqual(seen, [3]);
});

test("a batch that throws reports its own error, not that of a subscriber", () => {
  const store = createStore({ a: 1 });
  store.subscribe("a", () => {
    throw new Error("subscriber");
  });
  assert.throws(
    () =>
      store.batch(() => {
        store.state.a = 2;
        throw new Error("batch");
      }),
    /batch/,
  );
  // Without an error of its own, the batch still tells of a broken subscriber.
  assert.throws(() => store.batch(() => (store.state.a = 3)), /subscriber/);
});
