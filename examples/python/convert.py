"""Run with an API-enabled wheel: python examples/python/convert.py report.pdf"""

import argparse

from anymd import convert


def main():
    parser = argparse.ArgumentParser(
        description="Convert one local file with the installed anymd binary"
    )
    parser.add_argument("file")
    parser.add_argument("--pages")
    args = parser.parse_args()
    document = convert(args.file, pages=args.pages)
    print(document.text, end="")
    print("Source:", document.source)


if __name__ == "__main__":
    main()
