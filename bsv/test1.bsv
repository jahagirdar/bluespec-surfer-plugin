import FIFO::*;
import StmtFSM::*;
import FIFO::*;
import Vector::*;
import SpecialFIFOs::*;
// ==============================================================
// 1. Enum
// ==============================================================

typedef enum {
      	ADD, SUB, AND, OR, XOR,
      	SLL, SRL, SRA,
      	BEQ, BNE, BLT, JAL, JALR,
      	LOAD, STORE,
      	CSR_RW, CSR_RS, CSR_RC,
      	FENCE, ECALL
} AluOp deriving (Bits, Eq, FShow, Bounded);

typedef enum { FETCH, DECODE, EXECUTE, MEM, WB, HALT } Stage deriving (Bits, Eq, FShow);

// ==============================================================
// 2. Simple Tagged Union
// ==============================================================

typedef union tagged {
      	Bit#(64)   RegData;
      	Bit#(64)   ImmData;
      	Bit#(64)   PcPlus4;
      	Bit#(64)   MemLoad;
      	void       InvalidOp;
} OperandValue deriving (Bits, Eq, FShow);

// ==============================================================
// 3. Complex Nested Tagged Union (real-world style)
// ==============================================================

typedef union tagged {
      	struct { Bit#(64) addr; Bit#(64) data; }  MemWrite;
      	struct { Bit#(64) addr; Bit#(3) size;   }  MemRead;
      	struct { Bit#(64) target;               }  BranchTaken;
      	struct { Bit#(64) target; Bit#(5) rd;   }  JumpLink;
      	void                                      Exception;
} CommitAction deriving (Bits, Eq, FShow);

// ==============================================================
// 4. Ultimate Nested Monster Type (struct + enum + union + vector + maybe)
// ==============================================================

typedef struct {
      	Bit#(3)           core_id;
      	Stage             current_stage;
      	AluOp             alu_op;
      	Vector#(3, Bit#(5)) gpr_read_ports;
      	Maybe#(Bit#(64))  branch_target;
      	OperandValue      operand_a;
      	OperandValue      operand_b;
      	CommitAction      commit_action;
} SuperPacket deriving (Bits, Eq, FShow);

// ==============================================================
// 5. Instantiate Reg / Wire / RWire for ALL these types
// ==============================================================


typedef enum{
	Red=1,
	Blue=20,
	Green,
	Black=40 
} Colors_e deriving(Bits, Eq, FShow);
typedef struct{
	Colors_e rgb;
	Bit#(8) b;
} Foo_st deriving(Bits, Eq, FShow);
typedef struct {
	Bit#(50) fifty;
	Bit#(99) nn;
}BitLarge deriving(Bits, Eq, FShow);

typedef struct {
	Foo_st f;
	Bit#(10)a;
	Foo_st t;
} Bar_st deriving(Bits, Eq, FShow);

interface Ifc_a;
	method Action in(Bit#(32) x, Bar_st b);
	method Foo_st out;
endinterface

(*synthesize*)
module mkA(Ifc_a);
	Reg#(Bar_st) rb <-mkRegA(unpack(0));
	method Action in(Bit#(32) x, Bar_st b);
	endmethod
	method Foo_st out;
		return unpack(0);
	endmethod

endmodule
module mkB(Ifc_a);
	Reg#(Bar_st) bar_ax <-mkRegA(unpack(0));
	Ifc_a inst_a <- mkA();
	FIFO#(Foo_st) ff <-mkFIFO();
	method Action in(Bit#(32) x, Bar_st b);
	endmethod
	method Foo_st out;
		return unpack(0);
	endmethod

endmodule
(*synthesize*)
module mkTop(Empty);
	Reg#(Foo_st) rb <-mkRegA(unpack(0));
	Reg#(BitLarge) llarge <-mkRegA(unpack(0));
	Ifc_a aa <-mkB();
	Ifc_a ab <-mkB();
	Ifc_a ac <-mkA();
   	// Enums
   	Reg#(AluOp)      r_aluop   <- mkReg(ADD);
   	Wire#(AluOp)     w_aluop   <- mkWire;
   	RWire#(AluOp)    rw_aluop  <- mkRWire;

   	Reg#(Stage)      r_stage   <- mkReg(FETCH);
   	Wire#(Stage)     w_stage   <- mkWire;
   	RWire#(Stage)    rw_stage  <- mkRWire;
   	Reg#(Maybe#(Bit#(8))) mbe <-mkRegA(tagged Invalid);

   	// Simple union
   	Reg#(OperandValue)  r_opval   <- mkReg(tagged InvalidOp);
   	Wire#(OperandValue) w_opval   <- mkWire;
   	RWire#(OperandValue)rw_opval  <- mkRWire;

   	// Complex union
   	Reg#(CommitAction)  r_commit   <- mkReg(tagged Exception);
   	Wire#(CommitAction) w_commit   <- mkWire;
   	RWire#(CommitAction)rw_commit  <- mkRWire;

   	// The ultimate type
   	Reg#(SuperPacket)   r_super   <- mkRegU;
   	Wire#(SuperPacket)  w_super   <- mkWire;
   	RWire#(SuperPacket) rw_super  <- mkRWire;

   	// ==============================================================
   	// 6. FIFOs carrying enums, unions, and the monster type
   	// ==============================================================

   	FIFO#(AluOp)              fifo_aluop     <- mkBypassFIFO;
   	FIFO#(OperandValue)       fifo_operand   <- mkPipelineFIFO;
   	FIFO#(CommitAction)       fifo_commit    <- mkFIFO;
   	FIFO#(SuperPacket)        fifo_super     <- mkSizedFIFO(8);  // 8-deep

   	FIFO#(Stage)      fifo_stage4    <- mkSizedFIFO(4);

   	// ==============================================================
   	// 7. Counter and sample data generation
   	// ==============================================================

   	Reg#(Bit#(4)) cycle <- mkReg(0);
	rule incCycle;
		cycle <=cycle+1;
	endrule
	let s=seq
		rb<= Foo_st{rgb:Red,b:'h55};
		rb<= Foo_st{rgb:Black,b:'hff};
		rb<= Foo_st{rgb:Green,b:'h00};
		$display("End of simulation");
		$finish();
	endseq;
	FSM fsm <-mkFSM(s);
	let s1_blue=seq 
		rb<= Foo_st{rgb:Blue,b:'hab};
		rb<= Foo_st{rgb:Green,b:'h00};
	endseq;
	let s2_black=seq 
		rb<= Foo_st{rgb:Black,b:'hab};
		rb<= Foo_st{rgb:Green,b:'h00};
	endseq;
	let s3_green=seq 
		rb<= Foo_st{rgb:Green,b:'hab};
		rb<= Foo_st{rgb:Green,b:'h00};
	endseq;
	let states=seq 
		s1_blue;
		if(cycle==2) s2_black;
			if(cycle==1)	s3_green;
	endseq;
	FSM myfsm <-mkFSM(states);
	rule start;
		myfsm.start();
		mbe <= tagged Valid 0;
	endrule

endmodule
