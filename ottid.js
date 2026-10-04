/* Ottid, the creature on the landing page.
 *
 * A light port of the reference prototype (docs/design/ottid/puddle.html on
 * main): the taskbar placement in the idle state. The page ships a static pose
 * that works without this script. The script animates it only when the visitor
 * allows motion, and only while the stage is on screen. Everything moves on
 * damped springs, a = -w^2 (x - target) - 2 z w v, in fixed 240 Hz sub-steps.
 */
(() => {
  'use strict';

  const svg = document.querySelector('.ottid');
  if (!svg) return;
  const stage = svg.closest('.stage');
  const pet = stage && stage.querySelector('.pet');
  const reduce = window.matchMedia('(prefers-reduced-motion: reduce)');

  const RX = 29, RY = 20.2, R = 22, FLAT = 11;
  const ER = 3.3, EH = 5, SEP = 8;

  const pick = (sel) => [-1, 1].map((sd) => svg.querySelector(`${sel}[data-side="${sd}"]`));
  const els = {
    move: svg.querySelector('.o-move'),
    body: svg.querySelector('.o-body'),
    arms: pick('.o-arm'),
    palms: pick('.o-palm'),
    eyes: pick('.o-eye'),
    lids: pick('.o-lid'),
    smiles: pick('.o-smile'),
  };
  if (!els.move || !els.body) return;

  // The static pose, restored when motion is switched off mid-animation.
  const touched = [els.move, els.body, ...els.arms, ...els.palms, ...els.eyes, ...els.lids, ...els.smiles];
  const statics = touched.map((el) => [el, ['transform', 'd', 'x1', 'y1', 'x2', 'y2', 'cx', 'cy', 'rx', 'ry']
    .map((a) => [a, el.getAttribute(a)])]);
  const restore = () => statics.forEach(([el, attrs]) => attrs.forEach(([a, v]) =>
    v === null ? el.removeAttribute(a) : el.setAttribute(a, v)));

  const spring = (x, response, damping) => ({ x, v: 0, t: x, w: 2 * Math.PI / response, z: damping });
  const step = (s, dt) => {
    const n = Math.max(1, Math.ceil(dt * 240)), h = dt / n;
    for (let i = 0; i < n; i++) {
      const a = -s.w * s.w * (s.x - s.t) - 2 * s.z * s.w * s.v;
      s.v += a * h;
      s.x += s.v * h;
    }
  };
  const clamp = (x, a, b) => Math.max(a, Math.min(b, x));
  const f = (n) => n.toFixed(2);

  const arm = (sd) => ({ sd, sx: spring(sd * 24, .3, .6), sy: spring(1, .3, .6), hx: spring(sd * 24, .22, .5), hy: spring(1, .22, .5) });
  const sim = {
    ph: .7,
    rot: spring(0, 1.1, .45), sq: spring(1, .38, .25), hop: spring(0, .42, .38), jig: spring(0, .5, .3),
    lt: spring(.12, .12, .9), lb: spring(0, .18, .9), sc: spring(1, .25, .7),
    gx: spring(0, .22, .85), gy: spring(0, .22, .85),
    arms: [arm(-1), arm(1)],
    blink: 0, nextBlink: 1.5, nextFidget: 4, happyUntil: -1,
    lookUntil: -1, look: [0, 0], waveUntil: -1, waveSide: 1,
  };
  let pointer = null, pointerAt = 0;

  function wave(T, dur = 1.8) {
    sim.waveUntil = T + dur;
    sim.waveSide = Math.random() < .5 ? -1 : 1;
  }
  function poke(T, k = 1) {
    sim.hop.v -= 150 * k;
    sim.sq.v += 1.6 * k;
    sim.jig.v += 3 * k;
    sim.happyUntil = T + .9;
  }

  // Arms rest on the taskbar; a waving arm swings up and out at the side.
  function armTargets(T) {
    const waving = T < sim.waveUntil;
    for (const a of sim.arms) {
      const shx = a.sd * 24, shy = 1;
      let e = -.6, L = 13;
      if (waving && a.sd === sim.waveSide) { e = .95 + .4 * Math.sin(T * 9); L = 20; }
      a.sx.t = shx; a.sy.t = shy;
      a.hx.t = shx + a.sd * L * Math.cos(e);
      a.hy.t = shy - L * Math.sin(e);
    }
  }

  function update(T, dt) {
    sim.nextFidget -= dt;
    if (sim.nextFidget <= 0) {
      const r = Math.random();
      if (r < .3) sim.jig.v += 3;
      else if (r < .6) { sim.look = [(Math.random() * 2 - 1) * .9, (Math.random() * 2 - 1) * .5]; sim.lookUntil = T + 1.3; }
      else if (r < .85) wave(T);
      else poke(T, .5);
      sim.nextFidget = 4 + Math.random() * 4;
    }
    armTargets(T);
    sim.rot.t = .025 * Math.sin(T * .9 + sim.ph);
    sim.sq.t = 1 + .02 * Math.sin(T * 1.6 + sim.ph);
    sim.lb.t = T < sim.happyUntil || T < sim.waveUntil ? .62 : 0;

    const near = pointer && performance.now() - pointerAt < 4000 && (() => {
      const r = svg.getBoundingClientRect();
      if (pointer[1] < r.top - 200 || pointer[1] > r.bottom + 200) return null;
      return [clamp((pointer[0] - r.left - r.width / 2) / 200, -1, 1), clamp((pointer[1] - r.top - r.height * .72) / 200, -1, 1)];
    })();
    if (near) { sim.gx.t = near[0]; sim.gy.t = near[1]; }
    else if (T < sim.lookUntil) { sim.gx.t = sim.look[0]; sim.gy.t = sim.look[1]; }
    else { sim.gx.t = .4 * Math.sin(T * .55 + sim.ph); sim.gy.t = .15 * Math.sin(T * .8 + sim.ph); }

    for (const k of ['rot', 'sq', 'hop', 'jig', 'lt', 'lb', 'sc', 'gx', 'gy']) step(sim[k], dt);
    for (const a of sim.arms) for (const k of ['sx', 'sy', 'hx', 'hy']) step(a[k], dt);
    sim.nextBlink -= dt;
    if (sim.nextBlink <= 0) { sim.blink = 1; sim.nextBlink = 2.4 + Math.random() * 3.6; }
    sim.blink = Math.max(0, sim.blink - dt * 7);
  }

  function render(T) {
    const sy = sim.sq.x, sx = 1 / Math.sqrt(sy), rot = sim.rot.x * 180 / Math.PI, hop = Math.min(0, sim.hop.x);
    els.move.setAttribute('transform',
      `translate(0 ${f(hop)}) rotate(${f(rot)} 0 ${FLAT}) translate(0 ${FLAT}) scale(${sx.toFixed(4)} ${sy.toFixed(4)}) translate(0 ${-FLAT})`);

    // The outline wobbles slowly on angular harmonics 2, 3 and 5.
    const wob = (.022 + .05 * Math.max(0, sim.jig.x)) * R;
    const t1 = 1.3 * T + sim.ph, t2 = -1.9 * T + 1, t3 = 2.7 * T + 2;
    let d = '';
    for (let i = 0; i < 72; i++) {
      const a = i / 72 * 2 * Math.PI;
      const w = wob * (.6 * Math.sin(2 * a + t1) + .5 * Math.sin(3 * a + t2) + .3 * Math.sin(5 * a + t3));
      d += `${i ? 'L' : 'M'}${f((RX + w) * Math.cos(a))} ${f((RY + w) * Math.sin(a))}`;
    }
    els.body.setAttribute('d', d + 'Z');

    sim.arms.forEach((a, i) => {
      const line = els.arms[i], palm = els.palms[i];
      if (line) { line.setAttribute('x1', f(a.sx.x)); line.setAttribute('y1', f(a.sy.x)); line.setAttribute('x2', f(a.hx.x)); line.setAttribute('y2', f(a.hy.x)); }
      if (palm) { palm.setAttribute('cx', f(a.hx.x)); palm.setAttribute('cy', f(a.hy.x)); }
    });

    // Ember eyes: they slide and foreshorten as if they sat on a sphere.
    const lt = Math.max(sim.lt.x, sim.blink > 0 ? 1 - Math.abs(Math.cos(sim.blink * Math.PI)) : 0);
    const gx = Math.sin(sim.gx.x * .9), gy = Math.sin(sim.gy.x * .9), lb = sim.lb.x;
    [-1, 1].forEach((side, i) => {
      const er = ER * sim.sc.x, eh = EH * sim.sc.x;
      const px = side * SEP + .72 * ER * gx, py = .4 * EH * gy;
      const rw = er * (1 - .22 * Math.abs(gx)), rh = eh * (1 - .15 * Math.abs(gy));
      const eye = els.eyes[i], lid = els.lids[i], smile = els.smiles[i];
      if (eye) { eye.setAttribute('cx', f(px)); eye.setAttribute('cy', f(py)); eye.setAttribute('rx', f(rw)); eye.setAttribute('ry', f(rh)); }
      const top = py - rh + lt * rh * 2;
      if (lid) lid.setAttribute('d', `M${f(px - 12)} ${f(top)}H${f(px + 12)}V30H${f(px - 12)}Z`);
      if (smile) {
        let path = `M${f(px - 12)} -40H${f(px + 12)}V40H${f(px - 12)}Z`;
        if (lb > .01) {
          const cy = py + rh + rh * 1.25 - lb * rh * 2 * .78, erx = rw * 2.1, ery = rh * 1.3;
          path += `M${f(px + erx)} ${f(cy)}A${f(erx)} ${f(ery)} 0 1 0 ${f(px - erx)} ${f(cy)}A${f(erx)} ${f(ery)} 0 1 0 ${f(px + erx)} ${f(cy)}Z`;
        }
        smile.setAttribute('d', path);
      }
    });
  }

  let raf = 0, last = 0, t0 = 0, now = 0, onScreen = true;

  function frame(ms) {
    const dt = Math.min(.05, (ms - last) / 1000);
    last = ms;
    now = (ms - t0) / 1000;
    update(now, dt);
    render(now);
    raf = requestAnimationFrame(frame);
  }
  function start() {
    if (raf) return;
    last = performance.now();
    if (!t0) {
      // Begin exactly where the static picture is: waving hello, smiling.
      t0 = last;
      sim.waveSide = 1;
      sim.waveUntil = 1.8;
      armTargets(0);
      for (const a of sim.arms) for (const k of ['sx', 'sy', 'hx', 'hy']) { a[k].x = a[k].t; a[k].v = 0; }
      sim.lb.x = .62;
    }
    if (pet) pet.hidden = false;
    raf = requestAnimationFrame(frame);
  }
  function stop() {
    cancelAnimationFrame(raf);
    raf = 0;
    if (pet) pet.hidden = true;
  }
  function sync() {
    if (!reduce.matches && onScreen) start();
    else stop();
    if (reduce.matches) { restore(); t0 = 0; }
  }

  if ('IntersectionObserver' in window) {
    new IntersectionObserver((entries) => {
      onScreen = entries[entries.length - 1].isIntersecting;
      sync();
    }).observe(svg);
  }
  if (reduce.addEventListener) reduce.addEventListener('change', sync);
  else if (reduce.addListener) reduce.addListener(sync);
  document.addEventListener('pointermove', (e) => { pointer = [e.clientX, e.clientY]; pointerAt = performance.now(); }, { passive: true });
  if (pet) pet.addEventListener('click', () => poke(now));
  sync();
})();
