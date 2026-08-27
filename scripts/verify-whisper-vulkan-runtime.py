#!/usr/bin/env python3
import sys

from whisper_runtime_verifier import VerificationError, main


if __name__ == "__main__":
    try:
        sys.exit(main())
    except VerificationError as error:
        print(f"verify-whisper-vulkan-runtime: {error}", file=sys.stderr)
        sys.exit(2)
