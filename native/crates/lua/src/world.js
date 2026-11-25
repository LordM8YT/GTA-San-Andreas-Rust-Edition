// World scripts in JavaScript: the same API as world.lua, on the same
// natives (Rust bridges __native(name, ...args) to them and returns their
// results as an array). Waiting is `await wait(seconds)`; a script body may
// use await at its top level.
'use strict';
const N = globalThis.__native;
delete globalThis.__native;

let now = 0;
const timers = [];
const handlers = {};

function report(error) {
  let text;
  if (error && error.stack) {
    const at = /:(\d+):?\d*\)?\s*$/m.exec(error.stack.split('\n')[1] || '');
    text = String(error.message !== undefined ? error.message : error) + (at ? ' (line ' + at[1] + ')' : '');
  } else {
    text = String(error);
  }
  N('error', text);
}

// Every body and handler runs on its own, so it can wait.
function start(fn, ...args) {
  try {
    const result = fn(...args);
    if (result && typeof result.then === 'function') {
      result.then(undefined, report);
    }
  } catch (error) {
    report(error);
  }
}

globalThis.wait = function (seconds) {
  seconds = Math.max(Number(seconds) || 0, 0);
  return new Promise((resolve) => timers.push({ at: now + seconds, resolve: () => resolve(seconds) }));
};
globalThis.spawn = function (fn, ...args) {
  if (typeof fn !== 'function') throw new Error('spawn needs a function');
  start(fn, ...args);
};
globalThis.delay = function (seconds, fn) {
  if (typeof fn !== 'function') throw new Error('delay needs a number of seconds and a function');
  start(async () => {
    await wait(seconds);
    await fn();
  });
};
globalThis.every = function (seconds, fn) {
  seconds = Number(seconds);
  if (!(seconds >= 0.05) || typeof fn !== 'function') throw new Error('every needs at least 0.05 seconds and a function');
  let running = true;
  start(async () => {
    while (running) {
      await wait(seconds);
      if (running) await fn();
    }
  });
  return { stop: () => { running = false; } };
};
globalThis.print = function (...parts) {
  N('print', parts.map((p) => (typeof p === 'string' ? p : String(p))).join('  '));
};

// Vectors: world metres (x east, y north, z up).
class Vector {
  constructor(x, y, z) {
    this.x = Number(x) || 0;
    this.y = Number(y) || 0;
    this.z = Number(z) || 0;
  }
  add(o) { return new Vector(this.x + o.x, this.y + o.y, this.z + o.z); }
  sub(o) { return new Vector(this.x - o.x, this.y - o.y, this.z - o.z); }
  mul(n) { return new Vector(this.x * n, this.y * n, this.z * n); }
  div(n) { return new Vector(this.x / n, this.y / n, this.z / n); }
  neg() { return new Vector(-this.x, -this.y, -this.z); }
  equals(o) { return o instanceof Vector && this.x === o.x && this.y === o.y && this.z === o.z; }
  length() { return Math.sqrt(this.x * this.x + this.y * this.y + this.z * this.z); }
  distance(o) { return this.sub(o).length(); }
  toString() { return 'vector(' + this.x.toFixed(2) + ', ' + this.y.toFixed(2) + ', ' + this.z.toFixed(2) + ')'; }
}
globalThis.vector = (x, y, z) => new Vector(x, y, z);
const isVector = (v) => v instanceof Vector;

// Colours: red, green and blue from 0 to 1 (up to 2 brightens).
class Color {
  constructor(r, g, b) { this.r = r; this.g = g; this.b = b; }
  equals(o) {
    const close = (a, b) => Math.abs(a - b) < 1e-4;
    return o instanceof Color && close(this.r, o.r) && close(this.g, o.g) && close(this.b, o.b);
  }
  toString() { return 'rgb(' + this.r.toFixed(2) + ', ' + this.g.toFixed(2) + ', ' + this.b.toFixed(2) + ')'; }
}
globalThis.rgb = function (r, g, b) {
  if (typeof r !== 'number' || typeof g !== 'number' || typeof b !== 'number') {
    throw new Error('rgb needs three numbers: red, green and blue from 0 to 1');
  }
  return new Color(r, g, b);
};

function check([ok, message]) {
  if (!ok) throw new Error(message);
}

// Objects of the world, by id; their fields read the world as it is now.
const objects = new Map();
function object(id) {
  if (id === undefined || id === null) return undefined;
  let o = objects.get(id);
  if (!o) {
    o = new WorldObject(id);
    objects.set(id, o);
  }
  return o;
}
function move(id, position, heading, seconds) {
  if (!isVector(position)) throw new Error('a position is a vector(x, y, z)');
  if (typeof heading !== 'number') throw new Error('a heading is a number of degrees');
  if (seconds !== undefined && typeof seconds !== 'number') throw new Error('seconds must be a number');
  check(N('move', id, position.x, position.y, position.z, heading, seconds || 0));
}
class WorldObject {
  constructor(id) { Object.defineProperty(this, 'id', { value: id, enumerable: true }); }
  get name() { return N('object_name', this.id)[0]; }
  get exists() { return N('object_name', this.id)[0] !== undefined; }
  get visible() { return N('visible', this.id)[0]; }
  set visible(value) { check(N('set_visible', this.id, !!value)); }
  get position() { const [x, y, z] = N('position', this.id); return new Vector(x, y, z); }
  set position(value) { move(this.id, value, N('target', this.id)[3], 0); }
  get heading() { return N('heading', this.id)[0]; }
  set heading(value) { const [x, y, z] = N('target', this.id); move(this.id, new Vector(x, y, z), value, 0); }
  get color() { const [r, g, b] = N('color', this.id); return r === undefined ? undefined : new Color(r, g, b); }
  set color(value) {
    if (value === undefined || value === null) return check(N('set_color', this.id));
    if (typeof value.r !== 'number' || typeof value.g !== 'number' || typeof value.b !== 'number') {
      throw new Error('a colour is rgb(red, green, blue) from 0 to 1, or null for its own');
    }
    check(N('set_color', this.id, value.r, value.g, value.b));
  }
  get original() { return object(N('original', this.id)[0]); }
  get tags() { return N('tags', this.id)[0] || []; }
  show() { this.visible = true; }
  hide() { this.visible = false; }
  toggle() { this.visible = !this.visible; }
  on(event, fn) { return world.on(event, this, fn); }
  moveTo(position, seconds, heading) {
    if (heading === undefined) heading = N('target', this.id)[3];
    move(this.id, position, heading, seconds);
  }
  turnTo(heading, seconds) {
    const [x, y, z] = N('target', this.id);
    move(this.id, new Vector(x, y, z), heading, seconds);
  }
  clone(position, heading) {
    if (position === undefined) { const [x, y, z] = N('position', this.id); position = new Vector(x, y, z); }
    else if (!isVector(position)) throw new Error('clone takes a position vector(x, y, z), or nothing for where it is');
    if (heading === undefined) heading = N('heading', this.id)[0];
    else if (typeof heading !== 'number') throw new Error('a heading is a number of degrees');
    const [copy, message] = N('clone', this.id, position.x, position.y, position.z, heading);
    if (copy === undefined) throw new Error(message);
    return object(copy);
  }
  destroy() {
    check(N('destroy', this.id));
    objects.delete(this.id);
  }
  get(name) {
    const [a, y, z] = N('attribute', this.id, String(name));
    return z !== undefined ? new Vector(a, y, z) : a;
  }
  set(name, value) {
    if (typeof name !== 'string') throw new Error('an attribute name is a text');
    if (isVector(value)) return check(N('set_attribute', this.id, name, value.x, value.y, value.z));
    const kind = typeof value;
    if (value === undefined || value === null || kind === 'boolean' || kind === 'number' || kind === 'string') {
      return check(N('set_attribute', this.id, name, value === null ? undefined : value));
    }
    throw new Error('an attribute is true/false, a number, a text or a vector (null removes it)');
  }
  hasTag(name) { return N('has_tag', this.id, String(name))[0]; }
  addTag(name) { check(N('set_tag', this.id, String(name), true)); }
  removeTag(name) { check(N('set_tag', this.id, String(name), false)); }
  toString() { return 'object ' + this.name; }
}

// Players in the world, by id.
const players = new Map();
function player(id) {
  if (id === undefined || id === null) return undefined;
  let p = players.get(id);
  if (!p) {
    p = new Player(id);
    players.set(id, p);
  }
  return p;
}
class Player {
  constructor(id) { Object.defineProperty(this, 'id', { value: id, enumerable: true }); }
  get name() { return N('player_name', this.id)[0]; }
  get here() { return N('player_name', this.id)[0] !== undefined; }
  get position() { const [x, y, z] = N('player_position', this.id); return x === undefined ? undefined : new Vector(x, y, z); }
  get driving() { return N('player_driving', this.id)[0]; }
  hud(name, text) {
    if (typeof name !== 'string' && typeof name !== 'number') throw new Error('hud needs a name for the line');
    check(N('hud', this.id, String(name), text === undefined || text === null ? undefined : String(text)));
  }
  message(text, seconds) { N('message', this.id, String(text), Number(seconds) || 4); }
  teleport(to, heading) {
    if (to instanceof WorldObject) return check(N('teleport', this.id, to.id));
    if (isVector(to)) return check(N('teleport_to', this.id, to.x, to.y, to.z, Number(heading) || 0));
    throw new Error('teleport needs an object or a vector');
  }
  toString() { return 'player ' + this.name; }
}

const builtin = new Set(['enter', 'leave', 'use', 'checkpoint', 'finish', 'join', 'quit']);
const world = {
  find: (name) => object(N('find', String(name))[0]),
  objects: () => (N('objects')[0] || []).map(object),
  tagged: (name) => (N('tagged', String(name))[0] || []).map(object),
  players: () => (N('players')[0] || []).map(player),
  message: (text, seconds) => { N('message', undefined, String(text), Number(seconds) || 4); },
  hud(name, text) {
    if (typeof name !== 'string' && typeof name !== 'number') throw new Error('hud needs a name for the line');
    check(N('hud', undefined, String(name), text === undefined || text === null ? undefined : String(text)));
  },
  hour: () => N('hour')[0],
  setHour: (hour) => check(N('set_hour', Number(hour))),
  weather: () => N('weather')[0],
  setWeather: (weather) => check(N('set_weather', String(weather))),
  time: () => now,
  // world.on(event, fn) or world.on(event, object or name, fn).
  on(event, filter, fn) {
    if (fn === undefined) { fn = filter; filter = undefined; }
    if (typeof event !== 'string' || typeof fn !== 'function') throw new Error('world.on needs an event name and a function');
    if (typeof filter === 'string') {
      const found = world.find(filter);
      if (!found) throw new Error('no object is named ' + filter);
      filter = found;
    }
    if (filter !== undefined && !(filter instanceof WorldObject)) {
      throw new Error('world.on can listen to an object, an object name, or everything');
    }
    const list = handlers[event] || (handlers[event] = []);
    const handler = { object: filter && filter.id, fn };
    list.push(handler);
    return { disconnect: () => { const i = list.indexOf(handler); if (i >= 0) list.splice(i, 1); } };
  },
  emit(name, ...values) {
    if (typeof name !== 'string' || builtin.has(name)) {
      throw new Error('world.emit needs a name of your own (not ' + String(name) + ')');
    }
    check(N('emit', name, values));
  },
};
globalThis.world = world;
globalThis.script = { name: N('script_name')[0], object: object(N('script_object')[0]) };

// Called by Rust.
globalThis.__world_tick = function (time) {
  now = time;
  const due = [];
  for (let i = timers.length - 1; i >= 0; i--) {
    if (timers[i].at <= now) due.push(timers.splice(i, 1)[0]);
  }
  due.reverse().forEach((t) => t.resolve());
};
globalThis.__world_event = function (name, objectId, playerId, ...rest) {
  const list = handlers[name];
  if (!list) return;
  let args;
  if (name === 'enter' || name === 'leave' || name === 'use') args = [player(playerId), object(objectId)];
  else if (name === 'checkpoint' || name === 'finish') args = [player(playerId), ...rest];
  else if (name === 'join' || name === 'quit') args = [player(playerId)];
  else args = rest;
  for (const h of list.slice()) {
    if (h.object === undefined || h.object === objectId) start(h.fn, ...args);
  }
};
globalThis.__world_start = function (body) {
  start(body);
};
