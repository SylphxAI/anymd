export function GET() {
  // Keep old embeds working without publishing another copy of the PDF.
  return new Response(null, { status: 307, headers: { Location: '/samples/sample.pdf' } });
}
