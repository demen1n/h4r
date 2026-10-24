// Decompile functions by RVA (and, optionally, their callees) into one C file.
// Usage (headless postScript): Decompile.java <out.c> <depth> <rva> [rva ...]
//@category h4r

import java.io.PrintWriter;
import java.util.*;

import ghidra.app.decompiler.*;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;

public class Decompile extends GhidraScript {
	@Override
	protected void run() throws Exception {
		String[] args = getScriptArgs();
		int depth = Integer.parseInt(args[1]);
		long base = currentProgram.getImageBase().getOffset();
		DecompInterface ifc = new DecompInterface();
		ifc.openProgram(currentProgram);

		Set<Function> seen = new LinkedHashSet<>();
		Deque<Object[]> todo = new ArrayDeque<>();
		for (int i = 2; i < args.length; i++) {
			Function f = getFunctionAt(toAddr(base + Long.parseLong(args[i], 16)));
			if (f != null) todo.add(new Object[] { f, 0 });
		}
		try (PrintWriter w = new PrintWriter(args[0])) {
			while (!todo.isEmpty()) {
				Object[] t = todo.poll();
				Function f = (Function) t[0];
				int d = (Integer) t[1];
				if (!seen.add(f)) continue;
				DecompileResults r = ifc.decompileFunction(f, 120, monitor);
				Address e = f.getEntryPoint();
				w.printf("// rva %x  %s%n", e.getOffset() - base, f.getName(true));
				String c = r.getDecompiledFunction() != null ? r.getDecompiledFunction().getC() : "// decompile failed\n";
				w.println(c);
				if (d < depth)
					for (Function callee : f.getCalledFunctions(monitor))
						if (!callee.isThunk() && !callee.isExternal()) todo.add(new Object[] { callee, d + 1 });
			}
		}
		println("decompiled " + seen.size() + " functions -> " + args[0]);
	}
}
