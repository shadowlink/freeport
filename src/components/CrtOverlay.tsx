// Non-interactive Arcade/CRT texture layer (scanlines + grain + vignette).
// Styling lives in index.css (.crt); intensity via the --crt CSS variable.
export default function CrtOverlay() {
  return <div className="crt" aria-hidden="true" />;
}
