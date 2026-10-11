export function AppLogo({ className = "h-5 w-5" }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 256 256"
      className={className}
      fill="currentColor"
      xmlns="http://www.w3.org/2000/svg"
    >
      <title>AnyLeap Logo</title>
      <defs>
        <linearGradient id="logo-beam" x1="0" y1="1" x2="1" y2="0">
          <stop offset="0%" stopColor="#38bdf8" />
          <stop offset="100%" stopColor="#22d3ee" />
        </linearGradient>
      </defs>
      {/* Device Frame */}
      <path
        fill="#f4f4f5"
        d="M 46 72 C 46 51 63 34 84 34 L 126 34 C 132.6 34 138 39.4 138 46 C 138 52.6 132.6 58 126 58 L 84 58 C 75.2 58 68 65.2 68 74 L 68 182 C 68 190.8 75.2 198 84 198 L 156 198 C 164.8 198 172 190.8 172 182 L 172 142 C 172 135.4 177.4 130 184 130 C 190.6 130 196 135.4 196 142 L 196 182 C 196 203 179 220 158 220 L 84 220 C 63 220 46 203 46 182 Z"
      />
      {/* Leap Vector Arrow */}
      <path
        fill="url(#logo-beam)"
        d="M 106 150 L 178 78 L 150 78 C 143.4 78 138 72.6 138 66 C 138 59.4 143.4 54 150 54 L 214 54 C 220.6 54 226 59.4 226 66 L 226 130 C 226 136.6 220.6 142 214 142 C 207.4 142 202 136.6 202 130 L 202 102 L 130 174 C 125.3 178.7 117.7 178.7 113 174 C 108.3 169.3 108.3 161.7 113 157 Z"
      />
    </svg>
  );
}
