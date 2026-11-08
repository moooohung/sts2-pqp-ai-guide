using System.Collections.Immutable;
using System.Reflection.Metadata;
using System.Reflection.Metadata.Ecma335;
using System.Reflection.PortableExecutable;

// Metadata-only IL reader for the offline gate: every member a method body calls, loads as a
// function pointer, constructs or names with ldtoken, and every string literal it loads, with the
// calling method. The DLL is never loaded or run.
sealed record IlReference(string Caller, string Member, string Opcode);

static class IlScan
{
    // Operand sizes of the one-byte opcodes that carry one (ECMA-335 III); everything else has none.
    static int OperandSize(int op) => op switch {
        >= 0x0E and <= 0x13 or 0x1F or >= 0x2B and <= 0x37 or 0xDE => 1,
        0x20 or 0x22 or 0x27 or 0x28 or 0x29 or >= 0x38 and <= 0x44 or 0xDD or 0x6F or 0x70 or 0x71 or 0x72 or
            0x73 or 0x74 or 0x75 or 0x79 or >= 0x7B and <= 0x81 or 0x8C or 0x8D or 0x8F or 0xA3 or 0xA4 or 0xA5 or
            0xC2 or 0xC6 or 0xD0 => 4,
        0x21 or 0x23 => 8,
        _ => 0
    };
    static int OperandSize2(int op) => op switch {
        0x06 or 0x07 or 0x15 or 0x16 or 0x1C => 4,
        >= 0x09 and <= 0x0E => 2,
        0x12 or 0x19 => 1,
        _ => 0
    };

    internal static string TypeName(MetadataReader md, EntityHandle handle) => handle.Kind switch {
        HandleKind.TypeReference => Qualified(md, (TypeReferenceHandle)handle),
        HandleKind.TypeDefinition => Qualified(md, (TypeDefinitionHandle)handle),
        HandleKind.TypeSpecification => md.GetTypeSpecification((TypeSpecificationHandle)handle)
            .DecodeSignature(new Names(md), null),
        _ => handle.Kind.ToString()
    };
    static string Qualified(MetadataReader md, TypeReferenceHandle handle)
    {
        var type = md.GetTypeReference(handle);
        string name = md.GetString(type.Name);
        if (type.ResolutionScope.Kind == HandleKind.TypeReference)
            return Qualified(md, (TypeReferenceHandle)type.ResolutionScope) + "+" + name;
        string space = md.GetString(type.Namespace);
        return space.Length == 0 ? name : space + "." + name;
    }
    static string Qualified(MetadataReader md, TypeDefinitionHandle handle)
    {
        var type = md.GetTypeDefinition(handle);
        string name = md.GetString(type.Name);
        if (type.GetDeclaringType() is { IsNil: false } outer) return Qualified(md, outer) + "+" + name;
        string space = md.GetString(type.Namespace);
        return space.Length == 0 ? name : space + "." + name;
    }

    // "Namespace.Type::Member" for a method, field or member reference (generic arguments dropped,
    // so a rule names the definition).
    internal static string Member(MetadataReader md, EntityHandle handle)
    {
        switch (handle.Kind) {
            case HandleKind.MethodDefinition: {
                var method = md.GetMethodDefinition((MethodDefinitionHandle)handle);
                return Qualified(md, method.GetDeclaringType()) + "::" + md.GetString(method.Name);
            }
            case HandleKind.FieldDefinition: {
                var field = md.GetFieldDefinition((FieldDefinitionHandle)handle);
                return Qualified(md, field.GetDeclaringType()) + "::" + md.GetString(field.Name);
            }
            case HandleKind.MemberReference: {
                var member = md.GetMemberReference((MemberReferenceHandle)handle);
                string parent = member.Parent.Kind == HandleKind.MethodDefinition
                    ? Member(md, member.Parent) : Generic(TypeName(md, member.Parent));
                return parent + "::" + md.GetString(member.Name);
            }
            case HandleKind.MethodSpecification:
                return Member(md, md.GetMethodSpecification((MethodSpecificationHandle)handle).Method);
            default:
                return TypeName(md, handle);
        }
    }
    static string Generic(string type) => type.IndexOf('<') is var at and > 0 ? type[..at] : type;

    internal static List<IlReference> References(PEReader pe, MetadataReader md)
    {
        var found = new List<IlReference>();
        foreach (var typeHandle in md.TypeDefinitions)
        foreach (var methodHandle in md.GetTypeDefinition(typeHandle).GetMethods())
        {
            var method = md.GetMethodDefinition(methodHandle);
            if (method.RelativeVirtualAddress == 0) continue;
            string caller = Member(md, methodHandle);
            try { Scan(pe, md, method, caller, found); }
            catch (BadImageFormatException error) { throw new BadImageFormatException($"{caller}: {error.Message}", error); }
        }
        return found;
    }

    static void Scan(PEReader pe, MetadataReader md, MethodDefinition method, string caller, List<IlReference> found)
    {
        {
            var il = pe.GetMethodBody(method.RelativeVirtualAddress).GetILReader();
            while (il.RemainingBytes > 0)
            {
                int op = il.ReadByte();
                if (op == 0xFE)
                {
                    int op2 = il.ReadByte();
                    if (op2 is 0x06 or 0x07)
                        found.Add(new(caller, Member(md, MetadataTokens.EntityHandle(il.ReadInt32())), op2 == 0x06 ? "ldftn" : "ldvirtftn"));
                    else il.Offset += OperandSize2(op2);
                    continue;
                }
                switch (op) {
                    case 0x28 or 0x6F or 0x73 or 0x27:
                        found.Add(new(caller, Member(md, MetadataTokens.EntityHandle(il.ReadInt32())),
                            op switch { 0x28 => "call", 0x6F => "callvirt", 0x73 => "newobj", _ => "jmp" }));
                        break;
                    case 0x29:
                        il.ReadInt32();
                        found.Add(new(caller, "calli", "calli"));
                        break;
                    case 0x72:
                        found.Add(new(caller, "\"" + md.GetUserString(MetadataTokens.UserStringHandle(il.ReadInt32() & 0xFFFFFF)) + "\"", "ldstr"));
                        break;
                    case 0xD0:
                        var token = MetadataTokens.EntityHandle(il.ReadInt32());
                        found.Add(new(caller, token.Kind is HandleKind.TypeReference or HandleKind.TypeDefinition or
                            HandleKind.TypeSpecification ? TypeName(md, token) : Member(md, token), "ldtoken"));
                        break;
                    case 0x45:
                        int targets = (int)il.ReadUInt32();
                        il.Offset += 4 * targets;
                        break;
                    default:
                        il.Offset += OperandSize(op);
                        break;
                }
            }
        }
    }

    sealed class Names(MetadataReader md) : ISignatureTypeProvider<string, object?>
    {
        public string GetArrayType(string element, ArrayShape shape) => element + "[]";
        public string GetByReferenceType(string element) => element + "&";
        public string GetFunctionPointerType(MethodSignature<string> signature) => "fnptr";
        public string GetGenericInstantiation(string generic, ImmutableArray<string> arguments) =>
            generic + "<" + string.Join(",", arguments) + ">";
        public string GetGenericMethodParameter(object? context, int index) => "!!" + index;
        public string GetGenericTypeParameter(object? context, int index) => "!" + index;
        public string GetModifiedType(string modifier, string unmodified, bool required) => unmodified;
        public string GetPinnedType(string element) => element;
        public string GetPointerType(string element) => element + "*";
        public string GetPrimitiveType(PrimitiveTypeCode code) => code.ToString();
        public string GetSZArrayType(string element) => element + "[]";
        public string GetTypeFromDefinition(MetadataReader reader, TypeDefinitionHandle handle, byte kind) => Qualified(md, handle);
        public string GetTypeFromReference(MetadataReader reader, TypeReferenceHandle handle, byte kind) => Qualified(md, handle);
        public string GetTypeFromSpecification(MetadataReader reader, object? context, TypeSpecificationHandle handle, byte kind) =>
            md.GetTypeSpecification(handle).DecodeSignature(this, context);
    }
}
