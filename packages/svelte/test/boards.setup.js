import { buildBoardModule } from '../src/build.js';
import { demoBoard, hostileBoard, messyBoard } from './helpers.js';

/** @param {{ provide: (key: string, value: unknown) => void }} project */
export default function setup({ provide }) {
  provide('boards', {
    demo: buildBoardModule(demoBoard),
    hostile: buildBoardModule(hostileBoard),
    messy: buildBoardModule(messyBoard),
  });
}
