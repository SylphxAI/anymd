"""Local document loading only; no indexes, embeddings or model downloads.

python examples/python/loaders.py langchain report.pdf
python examples/python/loaders.py llamaindex report.pdf
"""

import argparse


def main():
    parser = argparse.ArgumentParser(
        description="Load one local document with an optional anymd adapter"
    )
    parser.add_argument("framework", choices=("langchain", "llamaindex"))
    parser.add_argument("file")
    args = parser.parse_args()
    if args.framework == "langchain":
        from anymd.langchain import AnyMDLoader

        document = AnyMDLoader(args.file).load()[0]
        print(document.page_content, end="")
    else:
        from anymd.llamaindex import AnyMDReader

        document = AnyMDReader().load_data(args.file)[0]
        print(document.text, end="")
    print("Metadata:", document.metadata)


if __name__ == "__main__":
    main()
