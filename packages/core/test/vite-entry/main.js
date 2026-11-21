import { init, parseBoard } from '../../src/index.js';

await init();
console.log(parseBoard([]).diagnostics.length);
