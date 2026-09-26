const serviceStates = [
  ["Database", "Unavailable"],
  ["Playback", "Unavailable"],
  ["AI", "Not configured"],
] as const;

export default function App() {
  return (
    <main className="app-shell">
      <section aria-labelledby="product-name">
        <p className="eyebrow">Local-first</p>
        <h1 id="product-name">FishMuse</h1>
        <p>Your local music companion is ready for its next connection.</p>
      </section>

      <section aria-label="Service status">
        <h2>Service status</h2>
        <ul className="service-list">
          {serviceStates.map(([service, state]) => (
            <li key={service}>
              {service}: {state}
            </li>
          ))}
        </ul>
      </section>
    </main>
  );
}
