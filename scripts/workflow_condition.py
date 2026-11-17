"""Bounded offline model for the job predicates tested here, not a scheduler.

Only Boolean/string equality, logical operators, context values and status
functions are supported. Unknown syntax or context fails instead of guessing.
Fixtures use typed booleans and strings; no numeric coercion is modeled.
"""
import ast
import re


def job(workflow, name):
    match = re.search(rf"^  {re.escape(name)}:\n(.*?)(?=^  [\w-]+:|\Z)", workflow, re.M | re.S)
    if not match:
        raise ValueError(f"missing job: {name}")
    return match.group(1)


def condition(workflow, name):
    match = re.search(r"^    if: (.+)$", job(workflow, name), re.M)
    if not match:
        raise ValueError(f"missing condition: {name}")
    return match.group(1)


def eligible(expression, context, *, success=True, cancelled=False, failure=False):
    expression = expression.strip()
    if expression.startswith("$" + "{{") and expression.endswith("}}"):
        expression = expression[3:-2].strip()
    values = {}
    has_status = False
    status = {"always()": True, "success()": success,
              "cancelled()": cancelled, "failure()": failure}

    def token(match):
        nonlocal has_status
        word = match.group(0)
        if word.startswith("'"):
            return repr(word[1:-1].replace("''", "'"))
        if word in status:
            has_status = True
            value = status[word]
        elif word in ("true", "false"):
            value = word == "true"
        elif word in context:
            value = context[word]
        else:
            raise ValueError(f"unsupported condition token: {word}")
        if type(value) not in (str, bool):
            raise ValueError(f"unsupported condition value: {word}")
        name = f"v{len(values)}"
        values[name] = value
        return name

    expression = re.sub(r"'(?:[^']|'')*'|[A-Za-z_][\w.-]*(?:\(\))?", token, expression)
    expression = expression.replace("&&", " and ").replace("||", " or ")
    expression = re.sub(r"!(?!=)", " not ", expression).strip()

    def value(node):
        if isinstance(node, ast.Name) and node.id in values:
            return values[node.id]
        if isinstance(node, ast.Constant) and type(node.value) is str:
            return node.value
        if isinstance(node, ast.BoolOp) and isinstance(node.op, (ast.And, ast.Or)):
            parts = [value(part) for part in node.values]
            if any(type(part) is not bool for part in parts):
                raise ValueError("logical operands must be Boolean")
            return all(parts) if isinstance(node.op, ast.And) else any(parts)
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.Not):
            operand = value(node.operand)
            if type(operand) is bool:
                return not operand
        if isinstance(node, ast.Compare) and len(node.ops) == 1:
            left, right = value(node.left), value(node.comparators[0])
            equal = type(left) is type(right) and left == right
            if isinstance(node.ops[0], ast.Eq):
                return equal
            if isinstance(node.ops[0], ast.NotEq):
                return not equal
        raise ValueError(f"unsupported condition syntax: {ast.dump(node)}")

    result = value(ast.parse(expression, mode="eval").body)
    if type(result) is not bool:
        raise ValueError("condition must be Boolean")
    return (has_status or success) and result
