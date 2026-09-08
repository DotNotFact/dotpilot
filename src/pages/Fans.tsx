import { Section } from "../components/ui";
import { useStore } from "../store";

export default function Fans() {
  const snap = useStore((s) => s.snap);
  return (
    <div className="flex flex-col gap-4">
      <header>
        <div className="eyebrow">Вентиляторы</div>
        <h1 className="text-[24px] mt-1">Кривые охлаждения — скоро</h1>
        <p className="text-ink-2 text-[12.5px] mt-1 max-w-[700px] leading-snug">Windows не даёт прямого доступа к оборотам корпусных и процессорных вентиляторов. План: подключить LibreHardwareMonitor (датчики + управление через SMBus/EC) и ATK HUB для материнской платы ASUS. Уже сейчас доступны обороты вентилятора видеокарты.</p>
      </header>
      <Section title="Что уже видно">
        <div className="text-[13px]">
          {snap?.gpu.available ? (
            <>
              Вентилятор видеокарты: <span className="num">{snap.gpu.fan_pct.toFixed(0)}%</span> при <span className="num">{snap.gpu.temp_c.toFixed(0)}°C</span>
            </>
          ) : (
            "Нет данных."
          )}
        </div>
      </Section>
      <Section title="План">
        <ul className="text-[12.5px] text-ink-2 list-disc ml-5 leading-relaxed">
          <li>Датчики: температура CPU (Tctl), VRM, чипсет, NVMe, обороты всех вентиляторов.</li>
          <li>Кривые «тихо / баланс / холод» для каждого профиля.</li>
          <li>Защита от троттлинга: предупреждение при Tctl выше 90°C и автоперевод в холодный профиль.</li>
        </ul>
      </Section>
    </div>
  );
}
