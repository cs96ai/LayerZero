/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        eth: '#627EEA',
        sol: '#9945FF',
        relay: '#14F195',
      },
    },
  },
  plugins: [require('@tailwindcss/typography')],
}
