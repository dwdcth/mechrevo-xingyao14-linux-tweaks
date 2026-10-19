/*
 * Intel ACPI Component Architecture
 * AML/ASL+ Disassembler version 20251212 (64-bit version)
 * Copyright (c) 2000 - 2025 Intel Corporation
 * 
 * Disassembling to symbolic ASL+ operators
 *
 * Disassembly of SSDT5.aml
 *
 * Original Table Header:
 *     Signature        "SSDT"
 *     Length           0x000000FA (250)
 *     Revision         0x02
 *     Checksum         0x5E
 *     OEM ID           "XXXXXX"
 *     OEM Table ID     "EDK2    "
 *     OEM Revision     0x00001000 (4096)
 *     Compiler ID      "ACPI"
 *     Compiler Version 0x00040000 (262144)
 */
DefinitionBlock ("", "SSDT", 2, "XXXXXX", "EDK2    ", 0x00001000)
{
    Scope (\_SB)
    {
        Name (MDSZ, 0x01E0)
        OperationRegion (PDSP, SystemIO, 0xB0, One)
        Field (PDSP, ByteAcc, NoLock, Preserve)
        {
            IOB2,   8
        }

        OperationRegion (GPCD, SystemMemory, 0x79C6B000, 0x0200)
        Field (GPCD, AnyAcc, NoLock, Preserve)
        {
            DTSZ,   32, 
            TKNO,   32, 
            PTYP,   8, 
            I_PS,   8, 
            PACT,   8, 
            RSVD,   40, 
            PGUI,   128, 
            Offset (0x20), 
            DBUF,   3840
        }

        Method (PCDE, 2, Serialized)
        {
            PGUI = Arg0
            TKNO = Arg1
            Local0 = Zero
            IOB2 = 0x17
            If ((I_PS == Zero))
            {
                If ((DTSZ <= MDSZ))
                {
                    Mid (DBUF, Zero, DTSZ, Local0)
                }
            }

            Return (Local0)
        }

        Method (HBID, 0, NotSerialized)
        {
            Local0 = \_SB.PCDE (Buffer (0x10)
                    {
                        /* 0000 */  0x83, 0x18, 0x1F, 0xC5, 0x00, 0xDF, 0x6A, 0x4F,  // ......jO
                        /* 0008 */  0x08, 0xA0, 0x36, 0x9F, 0x60, 0x98, 0xFD, 0xAF   // ..6.`...
                    }, 0x20000032)
            Return (Local0)
        }
    }
}

