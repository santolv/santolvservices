import assert from 'node:assert/strict';
import test from 'node:test';
import { mergeText } from '../src/ui/craft-merge.js';

test('Yjs combines edits within one WordCraft or DeckCraft text run', () => {
  assert.equal(mergeText('Inicio 😀 final', 'Nuevo inicio 😀 final', 'Inicio 😀 final añadido'),
    'Nuevo inicio 😀 final añadido');
  assert.equal(mergeText('a😀bc', 'abc', 'a😀bc!'), 'abc!');
  assert.equal(mergeText('a', 'ab', 'ab'), 'ab');
  assert.equal(mergeText('a medio z', 'X medio z!', 'X medio gran z'), 'X medio gran z!');
});
