// List functions whose entry lies in an RVA range: rva, size in bytes, name.
// Usage (headless postScript): FunctionsInRange.java <out.txt> <from rva> <to rva>
//@category h4r

import java.io.PrintWriter;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;

public class FunctionsInRange extends GhidraScript {
	@Override
	protected void run() throws Exception {
		String[] args = getScriptArgs();
		long base = currentProgram.getImageBase().getOffset();
		Address from = toAddr(base + Long.parseLong(args[1], 16));
		Address to = toAddr(base + Long.parseLong(args[2], 16));
		int n = 0;
		try (PrintWriter w = new PrintWriter(args[0])) {
			for (Function f : currentProgram.getFunctionManager().getFunctions(from, true)) {
				Address e = f.getEntryPoint();
				if (e.compareTo(to) >= 0) break;
				w.printf("%x\t%x\t%s%n", e.getOffset() - base, f.getBody().getNumAddresses(), f.getName(true));
				n++;
			}
		}
		println("listed " + n + " functions -> " + args[0]);
	}
}
