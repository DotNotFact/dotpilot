/**
 * Вычислительный тест видеокарты через WebGPU.
 *
 * Считает в WebView2, который и так входит в приложение, — это позволяет не тащить
 * в портативный exe графический стек ради одного теста.
 *
 * Проверка построена на детерминированной 32-битной целочисленной цепочке. Целые
 * числа выбраны сознательно: у них не бывает законных расхождений в последнем
 * разряде, поэтому любое отличие результата — это ошибка железа. Нестабильный
 * разгон гораздо чаще проявляется молча неверными числами, чем вылетом.
 *
 * Первый прогон становится эталоном, все последующие сверяются с ним побитно.
 * Дополнительно выборка из эталона проверяется тем же алгоритмом на процессоре —
 * это ловит случай, когда карта ошибается стабильно и потому «согласована сама с собой».
 */

// Типы WebGPU отсутствуют в стандартной библиотеке TypeScript, а тянуть
// @webgpu/types ради одного файла не хочется — отсюда точечные any.
/* eslint-disable @typescript-eslint/no-explicit-any */

const THREADS = 1 << 18; // 262 144 значений — 1 МБ на буфер
const WORKGROUP = 64;
const ROUNDS = 8192;

const SHADER = `
@group(0) @binding(0) var<storage, read_write> result: array<u32>;

fn step32(x0: u32) -> u32 {
  var x = x0;
  x = x ^ (x << 13u);
  x = x ^ (x >> 17u);
  x = x ^ (x << 5u);
  return x * 0x9E3779B1u + 0x85EBCA6Bu;
}

@compute @workgroup_size(${WORKGROUP})
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  var x = gid.x * 2654435761u + 1u;
  for (var i = 0u; i < ${ROUNDS}u; i = i + 1u) {
    x = step32(x);
  }
  result[gid.x] = x;
}
`;

/** Тот же шаг цепочки на процессоре — для независимой сверки эталона. */
function step32(x: number): number {
  x = (x ^ (x << 13)) >>> 0;
  x = (x ^ (x >>> 17)) >>> 0;
  x = (x ^ (x << 5)) >>> 0;
  return (Math.imul(x, 0x9e3779b1) + 0x85ebca6b) >>> 0;
}

function chain32(index: number): number {
  let x = (Math.imul(index, 2654435761) + 1) >>> 0;
  for (let i = 0; i < ROUNDS; i++) x = step32(x);
  return x;
}

export interface GpuTestProgress {
  seconds: number;
  dispatches: number;
  mismatches: number;
}

export interface GpuTestResult {
  available: boolean;
  /** Почему тест не удалось запустить. */
  reason?: string;
  adapter?: string;
  dispatches: number;
  mismatches: number;
  seconds: number;
  /** Тест доработал до конца, а не был прерван или сорван ошибкой. */
  completed: boolean;
}

/**
 * Гоняет вычислительную нагрузку заданное число секунд.
 *
 * @param seconds сколько длится проверка
 * @param onProgress вызывается примерно раз в секунду
 * @param signal позволяет прервать проверку
 */
export async function runGpuTest(
  seconds: number,
  onProgress?: (p: GpuTestProgress) => void,
  signal?: AbortSignal,
): Promise<GpuTestResult> {
  const empty = { dispatches: 0, mismatches: 0, seconds: 0, completed: false };

  const gpu = (navigator as any).gpu;
  if (!gpu) {
    return { available: false, reason: "WebGPU недоступен в этой сборке WebView2 — обновите Microsoft Edge WebView2 Runtime.", ...empty };
  }

  let device: any;
  let adapterName: string | undefined;
  try {
    const adapter = await gpu.requestAdapter({ powerPreference: "high-performance" });
    if (!adapter) return { available: false, reason: "WebGPU не выдал адаптер видеокарты.", ...empty };
    adapterName = adapter.info?.description || adapter.info?.device || undefined;
    device = await adapter.requestDevice();
  } catch (e) {
    return { available: false, reason: `Не удалось получить устройство WebGPU: ${String(e)}`, ...empty };
  }

  // Ошибка устройства (например, сброс драйвера) не должна выглядеть как успех.
  let deviceLost: string | null = null;
  device.lost.then((info: any) => {
    deviceLost = info?.message || "устройство WebGPU потеряно";
  });

  const bytes = THREADS * 4;
  const output = device.createBuffer({ size: bytes, usage: 0x80 | 0x4 | 0x8 }); // STORAGE | COPY_SRC | COPY_DST
  const staging = device.createBuffer({ size: bytes, usage: 0x1 | 0x8 }); // MAP_READ | COPY_DST

  const module = device.createShaderModule({ code: SHADER });
  const pipeline = device.createComputePipeline({ layout: "auto", compute: { module, entryPoint: "main" } });
  const bindGroup = device.createBindGroup({
    layout: pipeline.getBindGroupLayout(0),
    entries: [{ binding: 0, resource: { buffer: output } }],
  });

  const dispatchOnce = async (): Promise<Uint32Array> => {
    const enc = device.createCommandEncoder();
    const pass = enc.beginComputePass();
    pass.setPipeline(pipeline);
    pass.setBindGroup(0, bindGroup);
    pass.dispatchWorkgroups(THREADS / WORKGROUP);
    pass.end();
    enc.copyBufferToBuffer(output, 0, staging, 0, bytes);
    device.queue.submit([enc.finish()]);
    await staging.mapAsync(1); // GPUMapMode.READ
    const copy = new Uint32Array(staging.getMappedRange().slice(0));
    staging.unmap();
    return copy;
  };

  const started = performance.now();
  let dispatches = 0;
  let mismatches = 0;
  let completed = false;

  try {
    const reference = await dispatchOnce();
    dispatches++;

    // Независимая сверка выборки: ловит стабильно неверный результат,
    // который сам с собой согласован и потому не виден при сравнении прогонов.
    for (let i = 0; i < THREADS; i += Math.floor(THREADS / 64)) {
      if (reference[i] !== chain32(i)) mismatches++;
    }

    let lastReport = started;
    while (performance.now() - started < seconds * 1000) {
      if (signal?.aborted) break;
      if (deviceLost) throw new Error(deviceLost);

      const run = await dispatchOnce();
      dispatches++;
      for (let i = 0; i < THREADS; i++) {
        if (run[i] !== reference[i]) mismatches++;
      }

      const now = performance.now();
      if (onProgress && now - lastReport > 1000) {
        lastReport = now;
        onProgress({ seconds: (now - started) / 1000, dispatches, mismatches });
      }
    }
    completed = !signal?.aborted && !deviceLost;
  } catch (e) {
    return {
      available: true,
      adapter: adapterName,
      reason: `Проверка сорвалась: ${String(e)}`,
      dispatches,
      mismatches,
      seconds: (performance.now() - started) / 1000,
      completed: false,
    };
  } finally {
    output.destroy?.();
    staging.destroy?.();
    device.destroy?.();
  }

  return {
    available: true,
    adapter: adapterName,
    dispatches,
    mismatches,
    seconds: (performance.now() - started) / 1000,
    completed,
  };
}
