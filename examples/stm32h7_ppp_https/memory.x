/* STM32H743ZI: 2MB dual-bank flash, 512K AXI SRAM. Adjust LENGTH if your
 * exact part differs (check the datasheet's memory map). */
MEMORY
{
    FLASH : ORIGIN = 0x08000000, LENGTH = 2048K
    RAM   : ORIGIN = 0x24000000, LENGTH = 512K
}
