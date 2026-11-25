// The JS twin of the resolution policy against the same fixture as the
// crate (generated from the policy the templates shipped before the kit).
// Run: node --test js/test
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { ResolutionState, backingSize, defaultScale, initialAa, parseStoredScale, pickDefault } from '../settings.js';

const fixture = JSON.parse(
  readFileSync(fileURLToPath(new URL('../../fixtures/resolution.json', import.meta.url)), 'utf8'),
);

// ResolutionPolicy::default(), as the worker's `settings-options` sends it.
const POLICY = {
  minScale: 0.5,
  maxScale: 1.0,
  step: 0.05,
  pixelBudget: 2_800_000,
  minEffectiveDpr: 1,
  coarseStart: 0.6,
  softwareStart: 0.5,
  maxTexture: 8192,
};

const close = (a, b, what) => assert.ok(Math.abs(a - b) < 1e-9, `${what}: ${a} != ${b}`);

test('default scale matches the fixture', () => {
  for (const c of fixture.defaultScale) {
    const policy = { ...POLICY, pixelBudget: c.pixelBudget ?? POLICY.pixelBudget };
    const got = defaultScale(policy, c.cssW, c.cssH, c.dpr, { coarse: !!c.coarse, software: !!c.software });
    close(got, c.scale, JSON.stringify(c));
  }
});

test('backing size matches the fixture', () => {
  for (const c of fixture.backing) {
    const { width, height } = backingSize(c.nativeW, c.nativeH, c.scale, c.maxTexture);
    assert.deepEqual([width, height], c.size, JSON.stringify(c));
  }
});

test('state sequences match the fixture', () => {
  for (const c of fixture.state) {
    const state = new ResolutionState(POLICY, c.nativeW, c.nativeH, c.dpr, { coarse: !!c.coarse }, c.stored);
    c.steps.forEach((step, index) => {
      const at = `${c.name} step ${index}`;
      if (step.do === 'expect') {
        close(state.scale, step.scale, `${at} scale`);
        const { width, height } = state.backing();
        assert.deepEqual([width, height], step.size, `${at} size`);
      } else if (step.do === 'resize') {
        state.resized(step.nativeW, step.nativeH, step.dpr);
      } else if (step.do === 'gpuInfo') {
        assert.equal(state.gpuInfo(step), step.lowered, `${at} lowered`);
      } else if (step.do === 'slider') {
        state.setPercent(step.percent);
      }
    });
  }
});

test('stored scales', () => {
  assert.equal(parseStoredScale(POLICY, '0.80'), 0.8);
  assert.equal(parseStoredScale(POLICY, '0.3'), 0.5);
  assert.equal(parseStoredScale(POLICY, 'x'), null);
  assert.equal(parseStoredScale(POLICY, null), null);
});

test('initial mode: ?aa=, then stored, then the device default', () => {
  const options = {
    modes: ['off', 'smaa', 'msaa', 'taa', 'taa-quality', 'taa-balanced', 'taa-performance'].map((id) => ({ id })),
    defaults: { desktop: 'taa', coarse: 'off', software: 'off' },
  };
  const desktop = { coarse: false, software: false };
  assert.deepEqual(initialAa(options, desktop, null, null), { mode: 'taa', source: 'default' });
  assert.deepEqual(initialAa(options, { coarse: true }, null, null), { mode: 'off', source: 'default' });
  assert.deepEqual(initialAa(options, { software: true }, null, null), { mode: 'off', source: 'default' });
  assert.deepEqual(initialAa(options, desktop, 'msaa', null), { mode: 'msaa', source: 'stored' });
  assert.deepEqual(initialAa(options, desktop, 'msaa', 'taa-quality'), { mode: 'taa-quality', source: 'url' });
  assert.deepEqual(initialAa(options, desktop, 'msaa8', 'fxaa'), { mode: 'taa', source: 'default' });
  assert.equal(pickDefault(options.defaults, { coarse: true, software: true }), 'off');
});
